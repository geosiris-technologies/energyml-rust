//! Unstructured grids, in the shapes the corpus does not contain.
//!
//! The corpus holds one: fesapi's tetrahedron-plus-prism, two cells sharing one
//! face, everything well-formed. It is checked against measured normals in
//! `corpus.rs`. What it cannot check is what happens when a file is **wrong** —
//! and the ways an unstructured grid can be wrong are the ways it is worth
//! being careful about, because every one of them produces a plausible drawing
//! rather than an error.
//!
//! The cube here is built so its correct answer is obvious: an axis-aligned box
//! from the origin to (1, 1, 1), six quadrilateral faces, and each face's
//! stored winding chosen so that half of them need reversing. A reader that
//! ignored `CellFaceIsRightHanded` would get three faces right and three
//! inside out, which is exactly the failure that looks like a rendering bug.

use std::collections::HashMap;

use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect, Scalar};
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_0::{DataObjectReference, Hdf5Dataset};
use energyml_geometry::{Topology, geometry};
use energyml_resqml_2_0_1::{
    AbstractBooleanArray, AbstractIntegerArray, AbstractPoint3dArray, AbstractValueArray, BooleanHdf5Array,
    CellShape, IntegerHdf5Array, Point3dHdf5Array, ResqmlJaggedArray, UnstructuredGridGeometry,
    UnstructuredGridRepresentation,
};
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};

const CRS: &str = "22222222-2222-2222-2222-222222222222";
const PROXY: &str = "33333333-3333-3333-3333-333333333333";

/// The eight corners of the unit cube, in the order `0bZYX`.
///
/// So node `n` is at `(n & 1, (n >> 1) & 1, (n >> 2) & 1)` — which makes every
/// assertion below readable without a diagram.
fn cube_points() -> Vec<f64> {
    let mut values = Vec::new();
    for n in 0..8u32 {
        values.push((n & 1) as f64);
        values.push(((n >> 1) & 1) as f64);
        values.push(((n >> 2) & 1) as f64);
    }
    values
}

/// The cube's six faces, each wound **anticlockwise seen from outside**.
///
/// Written out rather than generated, because a generator would share whatever
/// mistake the reader has.
const OUTWARD_FACES: [[u32; 4]; 6] = [
    [0, 2, 3, 1], // z = 0, normal −z … reversed below
    [4, 5, 7, 6], // z = 1, normal +z
    [0, 1, 5, 4], // y = 0, normal −y
    [2, 6, 7, 3], // y = 1, normal +y
    [0, 4, 6, 2], // x = 0, normal −x
    [1, 3, 7, 5], // x = 1, normal +x
];

/// Which faces are stored the other way round, so the flag has to say so.
///
/// Alternating, so a reader that ignored the flags, or read them off by one,
/// or applied them to the wrong face, all fail differently.
const STORED_REVERSED: [bool; 6] = [true, false, true, false, true, false];

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

fn jagged(elements: &str, lengths: &str) -> ResqmlJaggedArray {
    ResqmlJaggedArray {
        elements: Some(AbstractValueArray::IntegerHdf5Array(IntegerHdf5Array {
            null_value: -1,
            values: dataset(elements),
        })),
        cumulative_length: Some(AbstractIntegerArray::IntegerHdf5Array(IntegerHdf5Array {
            null_value: -1,
            values: dataset(lengths),
        })),
    }
}

/// How the grid is built, so a test can spoil exactly one thing.
struct Cube {
    node_count: u64,
    face_count: u64,
    cell_count: u64,
    /// One flag per cell-face pair, unless a test shortens it.
    handedness: Vec<f64>,
    /// The nodes of each face, flattened.
    face_nodes: Vec<f64>,
}

impl Default for Cube {
    fn default() -> Self {
        let mut face_nodes = Vec::new();
        for (face, reversed) in OUTWARD_FACES.iter().zip(STORED_REVERSED) {
            let mut nodes = face.to_vec();
            if reversed {
                nodes.reverse();
            }
            face_nodes.extend(nodes.into_iter().map(f64::from));
        }
        Self {
            node_count: 8,
            face_count: 6,
            cell_count: 1,
            // The flag is "the stored winding already points out", so it is the
            // negation of "stored reversed".
            handedness: STORED_REVERSED
                .iter()
                .map(|reversed| if *reversed { 0.0 } else { 1.0 })
                .collect(),
            face_nodes,
        }
    }
}

impl Cube {
    fn build(self) -> (UnstructuredGridRepresentation, Store) {
        let mut store = Store::default();
        store.put("/Points", cube_points(), vec![8, 3]);
        store.put_flat("/NodesPerFace/elements", self.face_nodes);
        store.put_flat(
            "/NodesPerFace/cumulativeLength",
            (1..=6).map(|f| (f * 4) as f64).collect(),
        );
        store.put_flat("/FacesPerCell/elements", (0..6).map(f64::from).collect());
        store.put_flat("/FacesPerCell/cumulativeLength", vec![6.0]);
        store.put_flat("/CellFaceIsRightHanded", self.handedness);

        let grid = UnstructuredGridRepresentation {
            uuid: "11111111-1111-1111-1111-111111111111".to_string(),
            cell_count: self.cell_count,
            geometry: Some(UnstructuredGridGeometry {
                local_crs: dor(CRS),
                points: Some(AbstractPoint3dArray::Point3dHdf5Array(Point3dHdf5Array {
                    coordinates: dataset("/Points"),
                })),
                // Deliberately a lie, the way fesapi's own example grid declares
                // `prism` for a tetrahedron: nothing may read it.
                cell_shape: CellShape::Tetrahedral,
                node_count: self.node_count,
                face_count: self.face_count,
                nodes_per_face: jagged("/NodesPerFace/elements", "/NodesPerFace/cumulativeLength"),
                faces_per_cell: jagged("/FacesPerCell/elements", "/FacesPerCell/cumulativeLength"),
                cell_face_is_right_handed: Some(AbstractBooleanArray::BooleanHdf5Array(BooleanHdf5Array {
                    values: dataset("/CellFaceIsRightHanded"),
                })),
                ..Default::default()
            }),
            ..Default::default()
        };
        (grid, store)
    }
}

#[derive(Default)]
struct Store {
    datasets: HashMap<String, (Vec<f64>, Vec<i64>)>,
}

impl Store {
    fn put(&mut self, path: &str, values: Vec<f64>, shape: Vec<i64>) {
        self.datasets.insert(path.to_string(), (values, shape));
    }
    fn put_flat(&mut self, path: &str, values: Vec<f64>) {
        let shape = vec![values.len() as i64];
        self.datasets.insert(path.to_string(), (values, shape));
    }
}

impl ObjectSource for Store {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
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
                field_path: String::new(),
            })
            .into_iter()
            .collect()
    }
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        let (_, shape) = self.get(location)?;
        Ok(ArrayMetadata {
            dimensions: shape.clone(),
            element_type: "f64".to_string(),
        })
    }
    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        let (values, shape) = self.get(location)?;
        Ok(ArrayData {
            metadata: ArrayMetadata {
                dimensions: shape.clone(),
                element_type: "f64".to_string(),
            },
            values: ArrayValues::F64(values.clone()),
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
    fn get(&self, location: &ArrayLocation) -> energyml_domain::Result<&(Vec<f64>, Vec<i64>)> {
        self.datasets
            .get(&location.path_in_file)
            .ok_or_else(|| energyml_domain::Error::Missing(location.path_in_file.clone()))
    }
}

fn dataset_of(object: &dyn Reflect) -> Option<String> {
    if let Some(FieldValue::Scalar(Scalar::Str(path))) = object.field("PathInHdfFile") {
        return Some(path.to_string());
    }
    for field in object.type_info().fields {
        if let Some(FieldValue::Object(child)) = object.field(field.name)
            && let Some(found) = dataset_of(child)
        {
            return Some(found);
        }
    }
    None
}

/// The centre of a set of points.
fn centroid(points: &[[f64; 3]]) -> [f64; 3] {
    let mut sum = [0.0f64; 3];
    for point in points {
        for axis in 0..3 {
            sum[axis] += point[axis] / points.len() as f64;
        }
    }
    sum
}

fn newell(ring: &[[f64; 3]]) -> [f64; 3] {
    let mut normal = [0.0f64; 3];
    for index in 0..ring.len() {
        let (current, next) = (ring[index], ring[(index + 1) % ring.len()]);
        normal[0] += (current[1] - next[1]) * (current[2] + next[2]);
        normal[1] += (current[2] - next[2]) * (current[0] + next[0]);
        normal[2] += (current[0] - next[0]) * (current[1] + next[1]);
    }
    normal
}

/// The flag is applied, and applied per pair.
///
/// Three of the six faces are stored wound the wrong way; after
/// `faces_outward` all six normals point away from the cube's centre. Ignoring
/// the flag leaves three of them pointing in, which is the whole test.
#[test]
fn every_face_of_a_cube_comes_out_facing_outward() {
    let (grid, store) = Cube::default().build();
    let found = geometry::of(&grid, &store).expect("it evaluates");
    let patch = &found.patches[0];
    let points: Vec<[f64; 3]> = patch.points.clone();
    assert_eq!(points.len(), 8);

    let Topology::Polyhedra(cells) = &patch.topology else {
        panic!("an unstructured grid is polyhedra");
    };
    assert_eq!(cells.cell_count(), 1);
    assert_eq!(cells.face_count(), 6);
    assert_eq!(cells.slot_count(), 6, "nothing is shared in a one-cell grid");

    let centre = centroid(&points);
    let mut seen = 0;
    for face in cells.faces_outward(0) {
        let ring: Vec<[f64; 3]> = face.iter().map(|n| points[*n as usize]).collect();
        let normal = newell(&ring);
        let middle = centroid(&ring);
        let away = [
            middle[0] - centre[0],
            middle[1] - centre[1],
            middle[2] - centre[2],
        ];
        let dot = normal[0] * away[0] + normal[1] * away[1] + normal[2] * away[2];
        assert!(dot > 0.0, "face {face:?} points inward (dot {dot})");
        seen += 1;
    }
    assert_eq!(seen, 6);
}

/// The fan triangulation of a cube is twelve triangles, all facing out.
#[test]
fn the_triangulation_keeps_the_facing() {
    let (grid, store) = Cube::default().build();
    let found = geometry::of(&grid, &store).expect("it evaluates");
    let points = &found.patches[0].points;
    let Topology::Polyhedra(cells) = &found.patches[0].topology else {
        panic!("polyhedra");
    };
    let triangles = cells.triangles();
    assert_eq!(triangles.len(), 12, "two per quadrilateral face");

    let centre = centroid(points);
    for triangle in &triangles {
        let ring: Vec<[f64; 3]> = triangle.iter().map(|n| points[*n as usize]).collect();
        let normal = newell(&ring);
        let middle = centroid(&ring);
        let away = [
            middle[0] - centre[0],
            middle[1] - centre[1],
            middle[2] - centre[2],
        ];
        let dot = normal[0] * away[0] + normal[1] * away[1] + normal[2] * away[2];
        assert!(dot > 0.0, "triangle {triangle:?} points inward");
    }
}

/// One flag per **face** rather than per cell-face pair is refused, and the
/// refusal quotes the rule.
///
/// The two lengths agree on a grid whose cells share nothing, which is why this
/// mistake survives a synthetic test and destroys a real grid: every interior
/// face comes out inside out.
#[test]
fn a_handedness_array_of_the_wrong_length_is_refused() {
    let cube = Cube {
        handedness: vec![1.0; 5],
        ..Default::default()
    };
    let (grid, store) = cube.build();
    let error = geometry::of(&grid, &store).unwrap_err().to_string();
    assert!(
        error.contains("5 flag(s) for 6 cell-face pair(s)")
            && error.contains("sum of the cell face count per cell"),
        "the refusal should say the lengths and quote the rule: {error}"
    );
}

/// A face naming a node the grid does not have.
#[test]
fn a_node_index_past_the_node_count_is_refused() {
    let mut cube = Cube::default();
    cube.face_nodes[0] = 8.0;
    let (grid, store) = cube.build();
    let error = geometry::of(&grid, &store).unwrap_err().to_string();
    assert!(
        error.contains("node 8 of 8"),
        "the refusal should name the index and the count: {error}"
    );
}

/// `NodeCount` disagreeing with the points array. Both are in the file, and
/// when they disagree neither can be trusted.
#[test]
fn a_node_count_that_contradicts_the_points_is_refused() {
    let cube = Cube {
        node_count: 9,
        ..Default::default()
    };
    let (grid, store) = cube.build();
    let error = geometry::of(&grid, &store).unwrap_err().to_string();
    assert!(
        error.contains("`NodeCount` is 9") && error.contains("holds 8"),
        "the refusal should show both numbers: {error}"
    );
}

/// `FaceCount` disagreeing with what `NodesPerFace` describes.
#[test]
fn a_face_count_that_contradicts_the_faces_is_refused() {
    let cube = Cube {
        face_count: 7,
        ..Default::default()
    };
    let (grid, store) = cube.build();
    let error = geometry::of(&grid, &store).unwrap_err().to_string();
    assert!(
        error.contains("`FaceCount` is 7") && error.contains("describes 6"),
        "the refusal should show both numbers: {error}"
    );
}

/// A grid with a cell count and nothing else is legal — the schema advertises
/// it, "characterized by a cell count, and potentially nothing else" — and has
/// no shape at all. Named rather than drawn as nothing.
#[test]
fn a_grid_with_no_geometry_is_named_not_empty() {
    let (mut grid, store) = Cube::default().build();
    grid.geometry = None;
    let error = geometry::of(&grid, &store).unwrap_err().to_string();
    assert!(
        error.contains("cell count and no geometry"),
        "the refusal should say what is missing: {error}"
    );
}
