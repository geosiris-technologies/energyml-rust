//! Geometry derived from another representation, in the shapes the corpus does
//! not contain.
//!
//! Two families, and between them almost nothing to read from a real file. The
//! reference packages hold **one** local grid - the easy case: one interval per
//! axis, uniform children, the window starting at parent cell zero - and **no
//! sub-representation at all**. So a window at an offset, several intervals
//! with a different ratio each, coarsening rather than refinement,
//! `ChildCellWeights`, and every shape of `SubRepresentation` in either RESQML
//! version are built here by hand, the way `well.rs` builds the BRGM wellsite.
//!
//! The parent is deliberately **not** a box. Its pillars lean and its levels
//! are warped, because a construction that interpolates in the wrong space
//! agrees with the right one on a box and on nothing else.

use std::cell::Cell;
use std::collections::HashMap;

use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect, Scalar};
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_0::{DataObjectReference, Hdf5Dataset};
use energyml_geometry::{Topology, geometry};
use energyml_resqml_2_0_1::{
    AbstractBooleanArray, AbstractDoubleArray, AbstractIntegerArray, AbstractParentWindow,
    AbstractPoint3dArray, BooleanConstantArray, DoubleHdf5Array, IjkGridGeometry, IjkGridRepresentation,
    IjkParentWindow, IntegerHdf5Array, Intervals, KGaps, Point3dHdf5Array, Regrid,
};
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};

const PARENT: &str = "11111111-1111-1111-1111-111111111111";
const CRS: &str = "22222222-2222-2222-2222-222222222222";
const PROXY: &str = "33333333-3333-3333-3333-333333333333";
const POINTS: &str = "/parent/Points";

/// The parent: 4 × 2 × 2 cells, so 5 × 3 pillars over 3 levels.
const PARENT_NI: usize = 4;
const PARENT_NJ: usize = 2;
const PARENT_NK: usize = 2;

/// Every pillar leans and every level is warped, by amounts chosen so that no
/// two axes share a formula:
///
/// ```text
/// x = 100 i + 7 k        y = 50 j + 3 i        z = 200 k + 4 i + 9 j
/// ```
///
/// A reader that interpolated in the wrong space, transposed an axis or used
/// the wrong level gets a different number for every node.
fn expected(i: f64, j: f64, k: f64) -> [f64; 3] {
    [
        100.0 * i + 7.0 * k,
        50.0 * j + 3.0 * i,
        200.0 * k + 4.0 * i + 9.0 * j,
    ]
}

/// The same formula sampled at the integers: the parent's own array.
///
/// The oracle above is legitimate precisely because this lattice is
/// **trilinear by construction** - each coordinate is a sum of terms linear in
/// one index - so interpolating it and evaluating the formula must agree
/// exactly. That is what makes the assertions arithmetic rather than a second
/// implementation of the same code.
fn parent_points() -> Vec<f64> {
    let mut values = Vec::new();
    for k in 0..=PARENT_NK {
        for j in 0..=PARENT_NJ {
            for i in 0..=PARENT_NI {
                values.extend(expected(i as f64, j as f64, k as f64));
            }
        }
    }
    values
}

fn dor(uuid: &str) -> DataObjectReference {
    DataObjectReference {
        uuid: uuid.to_string(),
        ..Default::default()
    }
}

fn dataset(path: &str) -> Hdf5Dataset {
    Hdf5Dataset {
        path_in_hdf_file: path.to_string(),
        hdf_proxy: dor(PROXY),
    }
}

/// The same formula over the four levels of a parent with a **K gap after
/// layer 0**: layer 0 between levels 0 and 1, the void between 1 and 2, layer
/// 1 between 2 and 3 (`NKL = NK + gapCount + 1`, the `KGaps` documentation).
/// The formula's `k` is the level, so layer 1 sits at `k` 2..3 - and a reader
/// that forgot the gap would read it at 1..2, a different number everywhere.
fn gapped_points() -> Vec<f64> {
    let mut values = Vec::new();
    for level in 0..=PARENT_NK + 1 {
        for j in 0..=PARENT_NJ {
            for i in 0..=PARENT_NI {
                values.extend(expected(i as f64, j as f64, level as f64));
            }
        }
    }
    values
}

fn gapped_parent() -> IjkGridRepresentation {
    IjkGridRepresentation {
        k_gaps: Some(KGaps {
            count: Some(1),
            gap_after_layer: Some(AbstractBooleanArray::BooleanConstantArray(BooleanConstantArray {
                value: true,
                count: (PARENT_NK - 1) as u64,
            })),
        }),
        ..parent()
    }
}

fn parent() -> IjkGridRepresentation {
    IjkGridRepresentation {
        uuid: PARENT.to_string(),
        ni: PARENT_NI as u64,
        nj: PARENT_NJ as u64,
        nk: PARENT_NK as u64,
        geometry: Some(IjkGridGeometry {
            local_crs: dor(CRS),
            points: Some(AbstractPoint3dArray::Point3dHdf5Array(Point3dHdf5Array {
                coordinates: dataset(POINTS),
            })),
            grid_is_righthanded: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// A regrid axis: where the window starts on the parent, and one
/// `(parent cells, child cells)` pair per interval.
///
/// The counts go through HDF5 arrays rather than inline XML because RESQML
/// 2.0.1 has no inline integer array - which is itself worth exercising, since
/// it means every regrid in a real 2.0.1 file costs a read.
struct Axis {
    start: u64,
    intervals: Vec<(i64, i64)>,
    weights: Option<Vec<f64>>,
}

impl Axis {
    fn new(start: u64, intervals: &[(i64, i64)]) -> Self {
        Self {
            start,
            intervals: intervals.to_vec(),
            weights: None,
        }
    }

    fn weighted(mut self, weights: &[f64]) -> Self {
        self.weights = Some(weights.to_vec());
        self
    }

    /// Build the `Regrid`, registering its arrays in the store under names
    /// derived from `tag` so the three axes cannot collide.
    fn build(self, tag: &str, store: &mut Store) -> Regrid {
        let parents: Vec<f64> = self.intervals.iter().map(|(p, _)| *p as f64).collect();
        let children: Vec<f64> = self.intervals.iter().map(|(_, c)| *c as f64).collect();
        let count = self.intervals.len() as u64;
        store.put(&format!("/{tag}/parents"), parents);
        store.put(&format!("/{tag}/children"), children);
        let weights = self.weights.map(|weights| {
            store.put(&format!("/{tag}/weights"), weights);
            AbstractDoubleArray::DoubleHdf5Array(DoubleHdf5Array {
                values: dataset(&format!("/{tag}/weights")),
            })
        });
        Regrid {
            initial_index_on_parent_grid: self.start,
            intervals: Some(Intervals {
                interval_count: count,
                parent_count_per_interval: Some(AbstractIntegerArray::IntegerHdf5Array(IntegerHdf5Array {
                    null_value: -1,
                    values: dataset(&format!("/{tag}/parents")),
                })),
                child_count_per_interval: Some(AbstractIntegerArray::IntegerHdf5Array(IntegerHdf5Array {
                    null_value: -1,
                    values: dataset(&format!("/{tag}/children")),
                })),
                child_cell_weights: weights,
            }),
        }
    }
}

fn child(dimensions: [u64; 3], axes: [Axis; 3], store: &mut Store) -> IjkGridRepresentation {
    let [i, j, k] = axes;
    let (i, j, k) = (i.build("i", store), j.build("j", store), k.build("k", store));
    IjkGridRepresentation {
        uuid: "44444444-4444-4444-4444-444444444444".to_string(),
        ni: dimensions[0],
        nj: dimensions[1],
        nk: dimensions[2],
        parent_window: Some(AbstractParentWindow::IjkParentWindow(IjkParentWindow {
            parent_grid: dor(PARENT),
            i_regrid: i,
            j_regrid: j,
            k_regrid: k,
            ..Default::default()
        })),
        ..Default::default()
    }
}

/// A store holding the parent, and counting every value it hands out.
///
/// The count is the whole point of the type. A local grid that read its
/// parent's array whole and kept a corner would pass every assertion about
/// coordinates and quietly cost the model; here the reads are observed.
struct Store {
    datasets: HashMap<String, (Vec<f64>, Vec<i64>)>,
    /// Values handed out of the **points** array only - the one that is big.
    points_read: Cell<usize>,
    /// Reads of the points array.
    point_reads: Cell<usize>,
    /// Whether the parent handed out is [`gapped_parent`].
    gapped: bool,
}

impl Default for Store {
    fn default() -> Self {
        let mut datasets = HashMap::new();
        datasets.insert(
            POINTS.to_string(),
            (
                parent_points(),
                vec![
                    (PARENT_NK + 1) as i64,
                    ((PARENT_NI + 1) * (PARENT_NJ + 1)) as i64,
                    3,
                ],
            ),
        );
        Self {
            datasets,
            points_read: Cell::new(0),
            point_reads: Cell::new(0),
            gapped: false,
        }
    }
}

impl Store {
    /// A store whose parent has a K gap after layer 0, and one more level of
    /// points to show for it.
    fn gapped() -> Self {
        let mut store = Self::default();
        store.datasets.insert(
            POINTS.to_string(),
            (
                gapped_points(),
                vec![
                    (PARENT_NK + 2) as i64,
                    ((PARENT_NI + 1) * (PARENT_NJ + 1)) as i64,
                    3,
                ],
            ),
        );
        store.gapped = true;
        store
    }

    fn put(&mut self, path: &str, values: Vec<f64>) {
        let shape = vec![values.len() as i64];
        self.datasets.insert(path.to_string(), (values, shape));
    }
}

impl ObjectSource for Store {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        (uuid == PARENT)
            .then(|| Box::new(if self.gapped { gapped_parent() } else { parent() }) as Box<dyn DynObject>)
    }
}

impl ArraySource for Store {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        dataset_of(object)
            .map(|path| ArrayLocation {
                file: "memory".to_string(),
                path_in_file: path,
                mime: None,
                start: Vec::new(),
                count: Vec::new(),
                proxy_uuid: None,
                field_path: "Points".to_string(),
            })
            .into_iter()
            .collect()
    }

    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        let (_, shape) = self.dataset(location)?;
        Ok(ArrayMetadata {
            dimensions: shape.clone(),
            element_type: "f64".to_string(),
        })
    }

    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        let (whole, shape) = self.dataset(location)?;
        let (values, dimensions) = select(whole, shape, &location.start, &location.count);
        if location.path_in_file == POINTS {
            self.point_reads.set(self.point_reads.get() + 1);
            self.points_read.set(self.points_read.get() + values.len());
        }
        Ok(ArrayData {
            metadata: ArrayMetadata {
                dimensions,
                element_type: "f64".to_string(),
            },
            values: ArrayValues::F64(values),
        })
    }

    fn read_into(
        &self,
        location: &ArrayLocation,
        _out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        Ok(self.read(location)?.metadata)
    }
}

impl Store {
    fn dataset(&self, location: &ArrayLocation) -> energyml_domain::Result<&(Vec<f64>, Vec<i64>)> {
        self.datasets
            .get(&location.path_in_file)
            .ok_or_else(|| energyml_domain::Error::Missing(location.path_in_file.clone()))
    }
}

/// The hyperslab the storage layer would take, done in memory.
fn select(whole: &[f64], shape: &[i64], start: &[i64], count: &[i64]) -> (Vec<f64>, Vec<i64>) {
    if start.is_empty() || count.is_empty() {
        return (whole.to_vec(), shape.to_vec());
    }
    assert_eq!(shape.len(), 3, "only the points array is ever sliced");
    let mut out = Vec::new();
    for a in start[0]..start[0] + count[0] {
        for b in start[1]..start[1] + count[1] {
            for c in start[2]..start[2] + count[2] {
                out.push(whole[((a * shape[1] + b) * shape[2] + c) as usize]);
            }
        }
    }
    (out, count.to_vec())
}

/// The dataset an array names, under either version's spelling.
///
/// eml 2.0 calls it `PathInHdfFile` and eml 2.3 `PathInExternalFile` - the same
/// difference the real storage layer absorbs, reproduced here so the 2.2 tests
/// exercise the 2.2 spelling rather than a convenience.
fn dataset_of(object: &dyn Reflect) -> Option<String> {
    for field in ["PathInHdfFile", "PathInExternalFile"] {
        if let Some(FieldValue::Scalar(Scalar::Str(path))) = object.field(field) {
            return Some(path.to_string());
        }
    }
    for field in object.type_info().fields {
        match object.field(field.name) {
            Some(FieldValue::Object(child)) => {
                if let Some(found) = dataset_of(child) {
                    return Some(found);
                }
            }
            // eml 2.3 wraps the parts in a list: `ExternalDataArray` holds one
            // `ExternalDataArrayPart` per piece, and the path is inside it.
            Some(FieldValue::List(items)) => {
                for item in items {
                    if let Some(child) = item.as_object()
                        && let Some(found) = dataset_of(child)
                    {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn nodes(grid: &IjkGridRepresentation, store: &Store) -> Vec<[f64; 3]> {
    let found = geometry::of(grid, store).expect("the local grid evaluates");
    found.patches.into_iter().flat_map(|p| p.points).collect()
}

/// The plain case, to anchor the rest: one parent cell cut in two on I.
#[test]
fn a_window_of_one_cell_is_cut_where_the_regrid_says() {
    let mut store = Store::default();
    let grid = child(
        [2, 1, 1],
        [
            Axis::new(0, &[(1, 2)]),
            Axis::new(0, &[(1, 1)]),
            Axis::new(0, &[(1, 1)]),
        ],
        &mut store,
    );
    let points = nodes(&grid, &store);
    assert_eq!(points.len(), 3 * 2 * 2);
    // Child node 1 is halfway along parent cell 0 on I.
    assert_eq!(points[1], expected(0.5, 0.0, 0.0));
    assert_eq!(points[2], expected(1.0, 0.0, 0.0));
}

/// The window need not start at the origin, and `InitialIndexOnParentGrid` is
/// 0-based **on the parent grid** - the one sentence the schema spends on it.
#[test]
fn the_window_starts_where_the_initial_index_says() {
    let mut store = Store::default();
    let grid = child(
        [1, 1, 1],
        [
            Axis::new(2, &[(1, 1)]),
            Axis::new(1, &[(1, 1)]),
            Axis::new(1, &[(1, 1)]),
        ],
        &mut store,
    );
    let points = nodes(&grid, &store);
    assert_eq!(points.len(), 8);
    assert_eq!(points[0], expected(2.0, 1.0, 1.0));
    assert_eq!(points[7], expected(3.0, 2.0, 2.0));
}

/// Coarsening: the regrid runs both ways, and nothing in the corpus goes this
/// way.
#[test]
fn a_regrid_coarsens_as_well_as_refines() {
    let mut store = Store::default();
    let grid = child(
        [1, 1, 1],
        [
            Axis::new(0, &[(4, 1)]),
            Axis::new(0, &[(2, 1)]),
            Axis::new(0, &[(2, 1)]),
        ],
        &mut store,
    );
    let points = nodes(&grid, &store);
    assert_eq!(points.len(), 8);
    assert_eq!(points[0], expected(0.0, 0.0, 0.0));
    assert_eq!(points[7], expected(4.0, 2.0, 2.0));
}

/// The ratio may differ from one interval to the next, and the faces align at
/// the interval boundary: "Parent and child grid cell faces are aligned at
/// interval boundaries."
#[test]
fn each_interval_keeps_its_own_ratio() {
    let mut store = Store::default();
    let grid = child(
        [5, 1, 1],
        [
            // Two parent cells in one child, then two parent cells in four.
            Axis::new(0, &[(2, 1), (2, 4)]),
            Axis::new(0, &[(1, 1)]),
            Axis::new(0, &[(1, 1)]),
        ],
        &mut store,
    );
    let points = nodes(&grid, &store);
    assert_eq!(points.len(), 6 * 2 * 2);
    let along: Vec<f64> = points[..6].iter().map(|p| p[0]).collect();
    // 0, then the interval boundary at parent cell 2, then four equal steps of
    // half a parent cell to the end at 4.
    assert_eq!(along, vec![0.0, 200.0, 250.0, 300.0, 350.0, 400.0]);
}

/// `ChildCellWeights` are "proportional to the relative sizes of child cells
/// within each interval" and "need not be normalized" - so 1 : 3 puts the
/// interior node a quarter of the way along, not half.
#[test]
fn child_cell_weights_make_the_children_unequal() {
    let mut store = Store::default();
    let grid = child(
        [2, 1, 1],
        [
            Axis::new(0, &[(1, 2)]).weighted(&[1.0, 3.0]),
            Axis::new(0, &[(1, 1)]),
            Axis::new(0, &[(1, 1)]),
        ],
        &mut store,
    );
    let points = nodes(&grid, &store);
    assert_eq!(points[1], expected(0.25, 0.0, 0.0));
    assert_eq!(points[2], expected(1.0, 0.0, 0.0));
}

/// Only the window is read.
///
/// The parent holds 3 levels × 15 lines × 3 = 135 values. A window of one cell
/// on each axis needs 2 × 2 × 2 nodes, so 24 - taken in one hyperslab per row
/// of pillars, which is two reads. A regression to "evaluate the parent and
/// keep a corner" shows up here as 135 and nowhere else.
#[test]
fn a_local_grid_reads_only_its_window() {
    let mut store = Store::default();
    let grid = child(
        [2, 2, 2],
        [
            Axis::new(1, &[(1, 2)]),
            Axis::new(1, &[(1, 2)]),
            Axis::new(0, &[(1, 2)]),
        ],
        &mut store,
    );
    let points = nodes(&grid, &store);
    assert_eq!(points.len(), 27);

    let whole = (PARENT_NK + 1) * (PARENT_NI + 1) * (PARENT_NJ + 1) * 3;
    assert_eq!(whole, 135, "the parent's array, whole");
    assert_eq!(
        store.points_read.get(),
        24,
        "a 2×2×2 window is 8 points, and nothing else should have been read"
    );
    assert_eq!(
        store.point_reads.get(),
        2,
        "one hyperslab per row of pillars in the window"
    );
}

/// Below a K gap, the window starts at the top of its layer - a level, not
/// the layer's index. Layer 1 of the gapped parent is levels 2..3, so a child
/// cutting it in two on K has its nodes at levels 2, 2.5 and 3; a reader that
/// took the index for the level would put them at 1, 1.5 and 2, inside the
/// void and the layer above.
#[test]
fn below_a_k_gap_the_window_starts_at_its_layers_top() {
    let mut store = Store::gapped();
    let grid = child(
        [1, 1, 2],
        [
            Axis::new(1, &[(1, 1)]),
            Axis::new(0, &[(1, 1)]),
            Axis::new(1, &[(1, 2)]),
        ],
        &mut store,
    );
    let points = nodes(&grid, &store);
    assert_eq!(points.len(), 2 * 2 * 3);
    // Node order: i fastest, then j, then k - two nodes per row, four per level.
    for (ck, j, i) in [
        (0usize, 0usize, 0usize),
        (0, 0, 1),
        (0, 1, 1),
        (1, 1, 0),
        (2, 1, 1),
    ] {
        let at = ck * 4 + j * 2 + i;
        let level = 2.0 + ck as f64 / 2.0;
        assert_eq!(points[at], expected(1.0 + i as f64, j as f64, level), "node {at}");
    }
}

/// Across a K gap the child node between the two layers would have two
/// heights, so the window is refused and the gap named - the same refusal as
/// a window over a split coordinate line.
#[test]
fn a_window_across_a_k_gap_is_refused() {
    let mut store = Store::gapped();
    let grid = child(
        [1, 1, 2],
        [
            Axis::new(0, &[(1, 1)]),
            Axis::new(0, &[(1, 1)]),
            Axis::new(0, &[(1, 1), (1, 1)]),
        ],
        &mut store,
    );
    let error = geometry::of(&grid, &store).unwrap_err().to_string();
    assert!(
        error.contains("K gap") && error.contains("after parent layer 0"),
        "the refusal should name the gap: {error}"
    );
}

/// A window running off the edge of its parent is refused by name rather than
/// clamped. Clamping would put the refinement somewhere the file never said.
#[test]
fn a_window_past_the_edge_of_the_parent_is_refused() {
    let mut store = Store::default();
    let grid = child(
        [1, 1, 1],
        [
            Axis::new(4, &[(1, 1)]),
            Axis::new(0, &[(1, 1)]),
            Axis::new(0, &[(1, 1)]),
        ],
        &mut store,
    );
    let error = geometry::of(&grid, &store).unwrap_err().to_string();
    assert!(
        error.contains("4..5") && error.contains(" I,"),
        "the refusal should name the axis and the overrun: {error}"
    );
}

/// A regrid whose child counts do not add up to the child's own dimension is
/// the file contradicting itself, and the schema says so: "the sum of values
/// over all intervals must be equal to the corresponding child grid dimension".
#[test]
fn a_regrid_that_does_not_add_up_is_refused() {
    let mut store = Store::default();
    let grid = child(
        [5, 1, 1],
        [
            Axis::new(0, &[(1, 2)]),
            Axis::new(0, &[(1, 1)]),
            Axis::new(0, &[(1, 1)]),
        ],
        &mut store,
    );
    let error = geometry::of(&grid, &store).unwrap_err().to_string();
    assert!(
        error.contains("2 child cell(s)") && error.contains("dimension of 5"),
        "the refusal should say what does not add up: {error}"
    );
}

/// The cells are hexahedra with the child's own dimensions, so a renderer sees
/// a grid rather than a point cloud.
#[test]
fn a_local_grid_comes_out_as_cells() {
    let mut store = Store::default();
    let grid = child(
        [2, 2, 2],
        [
            Axis::new(0, &[(1, 2)]),
            Axis::new(0, &[(1, 2)]),
            Axis::new(0, &[(1, 2)]),
        ],
        &mut store,
    );
    let found = geometry::of(&grid, &store).expect("it evaluates");
    let Topology::Hexahedra {
        cells, dimensions, ..
    } = &found.patches[0].topology
    else {
        panic!("a local IJK grid is a set of hexahedra");
    };
    assert_eq!(*dimensions, [2, 2, 2]);
    assert_eq!(cells.len(), 8);
    for cell in cells {
        let mut sorted = cell.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 8, "a cell using a node twice has no volume");
    }
}

// ---------------------------------------------------------------------------
// Sub-representations
// ---------------------------------------------------------------------------

/// The support for every sub-representation below: the same 4 × 2 × 2 parent.
///
/// Its 45 nodes and 16 cells are the two things a sub-representation can pick
/// from here, and the leaning-pillar formula means every node it picks is
/// identifiable by its coordinates alone.
const SUPPORT: &str = PARENT;

fn indices(path: &str, values: &[i64], store: &mut Store) -> AbstractIntegerArray {
    store.put(path, values.iter().map(|v| *v as f64).collect());
    AbstractIntegerArray::IntegerHdf5Array(IntegerHdf5Array {
        null_value: -1,
        values: dataset(path),
    })
}

/// RESQML 2.0.1: one `SupportingRepresentation` on the object, and the element
/// kind inside each patch's `ElementIndices`.
fn sub_201(
    element: energyml_resqml_2_0_1::IndexableElements,
    picked: &[i64],
    store: &mut Store,
) -> energyml_resqml_2_0_1::SubRepresentation {
    let indices = indices("/sub/201", picked, store);
    energyml_resqml_2_0_1::SubRepresentation {
        uuid: "55555555-5555-5555-5555-555555555555".to_string(),
        supporting_representation: dor(SUPPORT),
        sub_representation_patch: vec![energyml_resqml_2_0_1::SubRepresentationPatch {
            patch_index: 0,
            count: picked.len() as u64,
            element_indices: vec![energyml_resqml_2_0_1::ElementIndices {
                indexable_element: element,
                indices: Some(indices),
            }],
        }],
        ..Default::default()
    }
}

/// RESQML 2.2: the element kind on the object, and a `SupportingRepresentation`
/// on each patch - so one sub-representation can draw from several supports,
/// which 2.0.1 cannot express at all.
fn sub_22(
    element: energyml_eml_2_3::IndexableElement,
    patches: &[&[i64]],
    store: &mut Store,
) -> energyml_resqml_2_2::SubRepresentation {
    let patches = patches
        .iter()
        .enumerate()
        .map(|(n, picked)| {
            let path = format!("/sub/22/{n}");
            store.put(&path, picked.iter().map(|v| *v as f64).collect());
            energyml_resqml_2_2::SubRepresentationPatch {
                indices: Some(energyml_eml_2_3::AbstractIntegerArray::IntegerExternalArray(
                    external_ints(&path),
                )),
                supporting_representation: dor22(SUPPORT),
            }
        })
        .collect();
    energyml_resqml_2_2::SubRepresentation {
        uuid: "66666666-6666-6666-6666-666666666666".to_string(),
        indexable_element: element,
        sub_representation_patch: patches,
        ..Default::default()
    }
}

fn dor22(uuid: &str) -> energyml_eml_2_3::DataObjectReference {
    energyml_eml_2_3::DataObjectReference {
        uuid: uuid.to_string(),
        ..Default::default()
    }
}

fn external_ints(path: &str) -> energyml_eml_2_3::IntegerExternalArray {
    energyml_eml_2_3::IntegerExternalArray {
        null_value: -1,
        values: energyml_eml_2_3::ExternalDataArray {
            external_data_array_part: vec![energyml_eml_2_3::ExternalDataArrayPart {
                path_in_external_file: path.to_string(),
                ..Default::default()
            }],
        },
        ..Default::default()
    }
}

/// Picking nodes gives back exactly those nodes, in the order asked for.
#[test]
fn a_sub_representation_of_nodes_is_the_nodes_it_names() {
    let mut store = Store::default();
    // Node index = (k × 3 + j) × 5 + i for a 5 × 3 × 3 node lattice.
    let sub = sub_201(
        energyml_resqml_2_0_1::IndexableElements::Nodes,
        &[0, 7, 44],
        &mut store,
    );
    let found = geometry::of(&sub, &store).expect("it evaluates");
    let points: Vec<_> = found.patches.iter().flat_map(|p| p.points.clone()).collect();
    assert_eq!(points.len(), 3);
    assert_eq!(points[0], expected(0.0, 0.0, 0.0));
    // 7 = (0 × 3 + 1) × 5 + 2
    assert_eq!(points[1], expected(2.0, 1.0, 0.0));
    // 44 = (2 × 3 + 2) × 5 + 4, the far corner
    assert_eq!(points[2], expected(4.0, 2.0, 2.0));
    assert_eq!(found.patches[0].topology, Topology::Points);
}

/// Picking cells keeps only the points those cells use.
///
/// Without the renumbering a sub-representation of two cells would carry all 45
/// of the support's nodes - the opposite of what selecting them was for.
#[test]
fn a_sub_representation_of_cells_carries_only_their_nodes() {
    let mut store = Store::default();
    let sub = sub_201(
        energyml_resqml_2_0_1::IndexableElements::Cells,
        &[0, 15],
        &mut store,
    );
    let found = geometry::of(&sub, &store).expect("it evaluates");
    let patch = &found.patches[0];
    // Two cells that share no corner: 16 distinct nodes, not 45.
    assert_eq!(patch.points.len(), 16);
    let Topology::Hexahedra {
        cells, dimensions, ..
    } = &patch.topology
    else {
        panic!("cells come out as hexahedra");
    };
    assert_eq!(cells.len(), 2);
    assert_eq!(*dimensions, [2, 1, 1], "a selection of cells is not a box");
    for cell in cells {
        assert!(
            cell.iter().all(|n| (*n as usize) < patch.points.len()),
            "every renumbered corner is inside the patch"
        );
    }
    // Cell 0 is the parent cell at the origin, so its corners are the eight
    // nodes of the unit box in index space.
    let mut corners: Vec<[f64; 3]> = cells[0].iter().map(|n| patch.points[*n as usize]).collect();
    corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mut wanted: Vec<[f64; 3]> = (0..8)
        .map(|n| expected((n & 1) as f64, ((n >> 1) & 1) as f64, ((n >> 2) & 1) as f64))
        .collect();
    wanted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(corners, wanted);
}

/// The 2.2 shape: the kind on the object, a support per patch, several patches.
#[test]
fn the_2_2_shape_puts_the_support_on_each_patch() {
    let mut store = Store::default();
    let sub = sub_22(
        energyml_eml_2_3::IndexableElement::Nodes,
        &[&[0, 1], &[44]],
        &mut store,
    );
    let found = geometry::of(&sub, &store).expect("it evaluates");
    assert_eq!(found.patches.len(), 2, "one patch in, one patch out");
    assert_eq!(found.patches[0].points.len(), 2);
    assert_eq!(found.patches[1].points.len(), 1);
    // The node offsets make the patches addressable as one buffer, which is
    // what every exporter downstream needs.
    assert_eq!(found.patches[0].node_offset, 0);
    assert_eq!(found.patches[1].node_offset, 2);
    assert_eq!(found.patches[1].points[0], expected(4.0, 2.0, 2.0));
}

/// An index outside the support is the file contradicting itself, and it is
/// named rather than panicking or quietly picking the wrong node.
#[test]
fn an_index_outside_the_support_is_refused() {
    let mut store = Store::default();
    let sub = sub_201(
        energyml_resqml_2_0_1::IndexableElements::Nodes,
        &[0, 45],
        &mut store,
    );
    let error = geometry::of(&sub, &store).unwrap_err().to_string();
    assert!(
        error.contains("45") && error.contains("45 `nodes`"),
        "the refusal should say the index and the count: {error}"
    );
}

/// Two `ElementIndices` on one patch is 2.0.1's way of listing *pairs* - faces
/// of cells, nodes of faces. Which member carries the geometry is not something
/// the file says, so it is refused rather than guessed.
#[test]
fn a_pairwise_sub_representation_is_refused_by_name() {
    let mut store = Store::default();
    let mut sub = sub_201(energyml_resqml_2_0_1::IndexableElements::Cells, &[0], &mut store);
    let extra = energyml_resqml_2_0_1::ElementIndices {
        indexable_element: energyml_resqml_2_0_1::IndexableElements::Faces,
        indices: Some(indices("/sub/pair", &[0], &mut store)),
    };
    sub.sub_representation_patch[0].element_indices.push(extra);
    let error = geometry::of(&sub, &store).unwrap_err().to_string();
    assert!(
        error.contains("pairs of elements"),
        "the refusal should name what it is: {error}"
    );
}

/// An element this build has no topology for is named, one kind at a time,
/// rather than coming back as an empty selection.
#[test]
fn an_element_with_no_geometry_here_is_named() {
    let mut store = Store::default();
    let sub = sub_201(energyml_resqml_2_0_1::IndexableElements::Edges, &[0], &mut store);
    let error = geometry::of(&sub, &store).unwrap_err().to_string();
    assert!(
        error.contains("edges") || error.contains("Edges"),
        "the refusal should name the element: {error}"
    );
}
