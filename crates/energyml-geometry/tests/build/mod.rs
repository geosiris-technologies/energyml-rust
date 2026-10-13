//! The objects the connection-set tests are about, built by hand.
//!
//! No package of the corpus holds a `GridConnectionSetRepresentation`, so the
//! grid and the set are written here. Kept in a module of their own because the
//! test file is about *what the reader decides*, and forty lines of struct
//! literals in the middle of it would bury that.

use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect, Scalar};
use energyml_eml_2_0::{DataObjectReference, Hdf5Dataset};
use energyml_resqml_2_0_1::{
    AbstractBooleanArray, AbstractIntegerArray, AbstractPoint3dArray, AbstractValueArray, BooleanHdf5Array,
    ConnectionInterpretations, GridConnectionSetRepresentation, IjkGridGeometry, IjkGridRepresentation,
    IntegerHdf5Array, Point3dHdf5Array, ResqmlJaggedArray,
};

pub const CRS: &str = "22222222-2222-2222-2222-222222222222";
pub const GRID: &str = "11111111-1111-1111-1111-111111111111";
pub const PROXY: &str = "33333333-3333-3333-3333-333333333333";
pub const POINTS: &str = "/grid/Points";
pub const DEFINED: &str = "/grid/CellGeometryIsDefined";
pub const CELLS: &str = "/set/CellIndexPairs";
pub const FACES: &str = "/set/LocalFacePerCellIndexPairs";
pub const WHICH: &str = "/set/GridIndexPairs";
pub const INTERP: &str = "/set/InterpretationIndices";
pub const LENGTHS: &str = "/set/CumulativeLength";

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

fn integers(path: &str) -> AbstractIntegerArray {
    AbstractIntegerArray::IntegerHdf5Array(IntegerHdf5Array {
        null_value: -1,
        values: dataset(path),
    })
}

/// Two cells along I, one along J and K: 3 × 2 pillars over 2 levels.
///
/// `defined` is `CellGeometryIsDefined` when the grid states one - the flag that
/// makes the reader drop a cell, and therefore the flag that makes a position
/// stop being the file's cell number.
pub fn grid(defined: bool) -> Box<IjkGridRepresentation> {
    Box::new(IjkGridRepresentation {
        uuid: GRID.to_string(),
        ni: 2,
        nj: 1,
        nk: 1,
        geometry: Some(IjkGridGeometry {
            local_crs: dor(CRS),
            points: Some(AbstractPoint3dArray::Point3dHdf5Array(Point3dHdf5Array {
                coordinates: dataset(POINTS),
            })),
            grid_is_righthanded: true,
            cell_geometry_is_defined: defined.then(|| {
                AbstractBooleanArray::BooleanHdf5Array(BooleanHdf5Array {
                    values: dataset(DEFINED),
                })
            }),
            ..Default::default()
        }),
        ..Default::default()
    })
}

/// A connection set over that grid.
///
/// `faces` and `which` are the two arrays the schema makes optional - absent is
/// the ordinary case for a block-centred grid, and the reader draws something
/// different for each.
pub fn connections(
    pairs: &[(i64, i64)],
    faces: Option<&[i64]>,
    which: Option<&[i64]>,
) -> Box<GridConnectionSetRepresentation> {
    Box::new(GridConnectionSetRepresentation {
        uuid: "44444444-4444-4444-4444-444444444444".to_string(),
        count: pairs.len() as u64,
        cell_index_pairs: Some(integers(CELLS)),
        local_face_per_cell_index_pairs: faces.map(|_| integers(FACES)),
        grid_index_pairs: which.map(|_| integers(WHICH)),
        grid: vec![dor(GRID)],
        ..Default::default()
    })
}

/// Give a set the interpretations its connections belong to.
///
/// `features` are the uuids named, in the file's order - the order the jagged
/// array's indices index. Each gets a title built from its uuid so a test can
/// tell one from another without a second lookup.
pub fn interpreted(
    mut set: Box<GridConnectionSetRepresentation>,
    features: &[&str],
) -> Box<GridConnectionSetRepresentation> {
    set.connection_interpretations = Some(ConnectionInterpretations {
        interpretation_indices: ResqmlJaggedArray {
            elements: Some(AbstractValueArray::IntegerHdf5Array(IntegerHdf5Array {
                null_value: -1,
                values: dataset(INTERP),
            })),
            cumulative_length: Some(integers(LENGTHS)),
        },
        feature_interpretation: features
            .iter()
            .map(|uuid| DataObjectReference {
                uuid: uuid.to_string(),
                title: format!("fault {uuid}"),
                ..Default::default()
            })
            .collect(),
    });
    set
}

/// The dataset an array points at, wherever the version put the path.
pub fn dataset_of(object: &dyn Reflect) -> Option<String> {
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

/// So a caller can hand the reader a `&dyn Reflect` without knowing the type.
pub fn boxed<T: DynObject + 'static>(object: Box<T>) -> Box<dyn DynObject> {
    object
}
