//! Packages fabricated in memory, and what a test needs to look at a picture. Nothing is read from a file.

use std::io::Cursor;
use std::sync::Arc;

use energyml_core::registry::DynObject;
use energyml_domain::package_index::PackageIndex;
use energyml_domain::palettes::Palettes;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_sheet::draw::{DisplayList, Item};
use energyml_storage::EpcStore;
use energyml_views::{ColourDoor, NoColourMaps, ViewContext};

pub const CRS: &str = "11111111-1111-4111-8111-111111111111";
pub const TRAJECTORY: &str = "22222222-2222-4222-8222-222222222222";
pub const FRAME: &str = "33333333-3333-4333-8333-333333333333";

pub fn uri(qualified_type: &str, uuid: &str) -> String {
    format!("eml:///{qualified_type}({uuid})")
}

pub fn frame_uri() -> String {
    uri("resqml22.WellboreFrameRepresentation", FRAME)
}

pub fn trajectory_uri() -> String {
    uri("resqml22.WellboreTrajectoryRepresentation", TRAJECTORY)
}

fn doubles(values: &[f64]) -> eml23::AbstractFloatingPointArray {
    eml23::AbstractFloatingPointArray::FloatingPointXmlArray(eml23::FloatingPointXmlArray {
        count_per_value: 1,
        values: values.to_vec(),
        ..Default::default()
    })
}

/// RESQML TRG 2.0.1 section 2.1 p. 10-11: offset (1000, 2000), z counting downward, metres, easting northing.
fn depth_crs() -> energyml_resqml_2_0_1::LocalDepth3dCrs {
    use energyml_eml_2_0 as eml20;
    energyml_resqml_2_0_1::LocalDepth3dCrs {
        uuid: CRS.to_string(),
        schema_version: "2.0".into(),
        x_offset: 1000.0,
        y_offset: 2000.0,
        z_offset: 0.0,
        areal_rotation: eml20::PlaneAngleMeasure {
            value: 0.0,
            uom: eml20::PlaneAngleUom::Dega,
        },
        projected_uom: eml20::LengthUom::M,
        vertical_uom: eml20::LengthUom::M,
        z_increasing_downward: true,
        projected_axis_order: eml20::AxisOrder2d::EastingNorthing,
        projected_crs: Some(eml20::AbstractProjectedCrs::ProjectedCrsEpsgCode(
            eml20::ProjectedCrsEpsgCode { epsg_code: 2154 },
        )),
        vertical_crs: Some(eml20::AbstractVerticalCrs::VerticalCrsEpsgCode(
            eml20::VerticalCrsEpsgCode { epsg_code: 5720 },
        )),
        ..Default::default()
    }
}

/// A straight inclined trajectory: the local point (0, 0, 0) to (`east`, 0, 100), over MD 0 to 100.
fn trajectory(east: f64) -> resqml22::WellboreTrajectoryRepresentation {
    use resqml22::{
        AbstractParametricLineGeometry, AbstractPoint3dArray, ParametricLineGeometry, Point3d,
        Point3dLatticeArray, Point3dLatticeDimension,
    };
    resqml22::WellboreTrajectoryRepresentation {
        uuid: TRAJECTORY.to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: "a trajectory".to_string(),
            ..Default::default()
        },
        md_interval: eml23::MdInterval {
            md_min: 0.0,
            md_max: 100.0,
            uom: eml23::LengthUomExt::M,
            datum: None,
            ..Default::default()
        },
        geometry: Some(AbstractParametricLineGeometry::ParametricLineGeometry(
            ParametricLineGeometry {
                local_crs: eml23::DataObjectReference {
                    uuid: CRS.to_string(),
                    qualified_type: "resqml20.obj_LocalDepth3dCrs".to_string(),
                    ..Default::default()
                },
                control_point_parameters: Some(doubles(&[0.0, 100.0])),
                control_points: Some(AbstractPoint3dArray::Point3dLatticeArray(Point3dLatticeArray {
                    all_dimensions_are_orthogonal: None,
                    origin: Point3d {
                        coordinate1: 0.0,
                        coordinate2: 0.0,
                        coordinate3: 0.0,
                    },
                    dimension: vec![Point3dLatticeDimension {
                        direction: Point3d {
                            coordinate1: east,
                            coordinate2: 0.0,
                            coordinate3: 100.0,
                        },
                        spacing: Some(doubles(&[1.0])),
                    }],
                })),
                knot_count: 2,
                line_kind_index: 1,
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

fn frame(node_md: &[f64]) -> resqml22::WellboreFrameRepresentation {
    resqml22::WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: "a frame".to_string(),
            ..Default::default()
        },
        node_count: node_md.len() as i64,
        node_md: Some(doubles(node_md)),
        trajectory: eml23::DataObjectReference {
            uuid: TRAJECTORY.to_string(),
            qualified_type: "resqml22.WellboreTrajectoryRepresentation".to_string(),
            title: "a trajectory".to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn empty_store() -> EpcStore {
    EpcStore::from_reader(
        Box::new(Cursor::new(
            energyml_storage::extract::empty_package(None).expect("an empty package"),
        )),
        "views.epc",
        Arc::new(energyml_schemas::all()),
    )
    .expect("it opens")
}

pub fn put(store: &mut EpcStore, object: impl DynObject + 'static) {
    store.put(Box::new(object)).expect("it goes in");
}

/// A package with a CRS, a straight trajectory going `east` metres east over 100 m of depth, and a frame whose
/// nodes are at `node_md`.
pub fn package(east: f64, node_md: &[f64]) -> EpcStore {
    let mut store = empty_store();
    put(&mut store, depth_crs());
    put(&mut store, trajectory(east));
    put(&mut store, frame(node_md));
    store
}

/// What a test draws with: the store, its index, no colour map, the built-in palettes.
pub struct Opened {
    pub store: EpcStore,
    pub index: PackageIndex,
    pub palettes: Palettes,
}

impl Opened {
    pub fn of(store: EpcStore) -> Self {
        let index = PackageIndex::build(&store, 0);
        Opened {
            store,
            index,
            palettes: Palettes::new(),
        }
    }

    pub fn context<'a>(&'a self, colours: &'a dyn ColourDoor) -> ViewContext<'a> {
        ViewContext {
            source: &self.store,
            index: &self.index,
            colours,
            palettes: &self.palettes,
        }
    }
}

/// A door that knows no map.
pub const NO_MAPS: NoColourMaps = NoColourMaps;

/// The points of every polyline of the elements of `kind` in the layer `layer`.
pub fn polylines(list: &DisplayList, layer: &str, kind: &str) -> Vec<Vec<[f64; 2]>> {
    list.layers
        .iter()
        .filter(|one| one.name == layer)
        .flat_map(|one| one.elements.iter())
        .filter(|element| element.kind == kind)
        .flat_map(|element| element.items.iter())
        .filter_map(|item| match item {
            Item::Polyline { points, .. } => Some(points.clone()),
            _ => None,
        })
        .collect()
}

/// The texts of a list, in drawing order.
#[allow(dead_code)] // for the views of the next lots, which read their labels
pub fn texts(list: &DisplayList) -> Vec<String> {
    list.layers
        .iter()
        .flat_map(|layer| layer.elements.iter())
        .flat_map(|element| element.items.iter())
        .filter_map(|item| match item {
            Item::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

/// The distance from a point to a polyline.
pub fn distance_to_line(point: [f64; 2], line: &[[f64; 2]]) -> f64 {
    line.windows(2)
        .map(|pair| energyml_geometry::vector::plan::distance_to_segment(point, pair[0], pair[1]))
        .fold(f64::INFINITY, f64::min)
}

/// The largest abscissa a text of the list reaches (its position plus its width measured by the sheet's own door)
/// must stay inside the page: nothing a view writes leaves the image.
#[allow(dead_code)] // each view's test file calls it, a build of one file alone would not
pub fn assert_texts_in_page(list: &DisplayList, what: &str) {
    use energyml_sheet::draw::text::{styled_width, x_span};
    let widest = energyml_sheet::draw::text::max_text_x(list).expect("the picture has a text");
    // The text that reaches furthest, named, so a failure says what to fix.
    let named = list
        .layers
        .iter()
        .flat_map(|layer| &layer.elements)
        .flat_map(|element| &element.items)
        .filter_map(|item| match item {
            Item::Text { at, text, style } if style.rotate == 0.0 => Some((
                x_span(at[0], style.anchor, styled_width(text, style))[1],
                text.as_str(),
            )),
            _ => None,
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, text)| text)
        .unwrap_or_default();
    assert!(
        widest <= list.size[0] + 1e-6,
        "{what}: a text reaches x = {widest:.2} on a page {} wide: {named:?}",
        list.size[0]
    );
}
