//! The geosteering section (`energyml_section::along_well`), WP09-F.
//!
//! Every hole is fabricated: a line through a CRS stated by the test, never a
//! file. The expected values are worked out by hand from the formulas of the
//! projections (`developed`: `x = s(md) + dn cos I`, `y = tvd(P) - dn sin I`,
//! with `I` the inclination and `s` the cumulated plan length) and from the
//! spec the test cites; the shape of each assertion is the spec's.
//!
//! Each test says how to break it, in the comment under its name.

use std::collections::BTreeMap;

use energyml_core::registry::DynObject;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_3 as eml23;
use energyml_geometry::around_well::projection::{VerticalSection, WellAxis, WellProjection, WellProjections};
use energyml_geometry::crs::{AxisOrder, AxisReading, Crs, CrsKind, VerticalDirection, VerticalQuantity};
use energyml_geometry::parametric::{Path, Vertical};
use energyml_geometry::well::PlacedPath;
use energyml_resqml_2_2 as resqml22;
use energyml_section::along_well::{
    AxisSignAsk, Boundaries, BoundaryAsk, BoundaryMode, Curtain, CurtainAsk, CurtainColumns, CurtainLayout,
    Model, PlaneAxisAsk, Request, SectionAsk, View, boundaries, curtain_cells,
};
use energyml_sheet::draw::list::{DisplayList, Item};

mod boundaries;
mod cells;
mod detect;
mod horizons;
mod image;
mod layout;
mod markers;
mod model;
mod pick;
mod sources;

pub(crate) const EN: Option<&str> = Some("easting northing");

/// A CRS counting z upwards, in metres, with the order of its horizontal axes stated or not.
pub(crate) fn crs(order: Option<&str>) -> Crs {
    Crs {
        uuid: "test".into(),
        kind: "LocalDepth3dCrs",
        x_offset: 0.0,
        y_offset: 0.0,
        z_offset: 0.0,
        areal_rotation: 0.0,
        vertical_direction: VerticalDirection::Up,
        vertical_quantity: VerticalQuantity::Length,
        horizontal_uom: Some("m".into()),
        vertical_uom: Some("m".into()),
        time_uom: None,
        projected: CrsKind::Absent,
        vertical: CrsKind::Absent,
        axes: order
            .and_then(AxisOrder::from_order)
            .map_or(AxisReading::NotStated, AxisReading::Stated),
    }
}

/// A line placed through [`crs`].
pub(crate) fn placed(
    kind: i64,
    control: Vec<[f64; 3]>,
    parameters: Vec<f64>,
    tangents: Option<Vec<[f64; 3]>>,
) -> PlacedPath {
    let path = Path::new(kind, control, parameters, tangents, Vertical::Up).expect("a valid line");
    PlacedPath::new(path, crs(EN), "m").expect("a placeable path")
}

/// A straight hole of 1000 m, `degrees` from the vertical, heading east.
pub(crate) fn inclined(degrees: f64) -> PlacedPath {
    let (sin, cos) = degrees.to_radians().sin_cos();
    placed(
        1,
        vec![[0.0, 0.0, 0.0], [1000.0 * sin, 0.0, -1000.0 * cos]],
        vec![0.0, 1000.0],
        None,
    )
}

/// Down 1000 m, then 1000 m east: horizontal (inclination 90 degrees) from MD 1000 on.
pub(crate) fn drain_east() -> PlacedPath {
    placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0], [1000.0, 0.0, -1000.0]],
        vec![0.0, 1000.0, 2000.0],
        None,
    )
}

/// Down 1000 m, then 1000 m north.
pub(crate) fn drain_north() -> PlacedPath {
    placed(
        1,
        vec![[0.0, 0.0, 0.0], [0.0, 0.0, -1000.0], [0.0, 1000.0, -1000.0]],
        vec![0.0, 1000.0, 2000.0],
        None,
    )
}

/// A quarter circle of `radius` metres, from straight down to due east (MD end = radius pi / 2).
pub(crate) fn build_up(radius: f64) -> PlacedPath {
    placed(
        5,
        vec![[0.0, 0.0, 0.0], [radius, 0.0, -radius]],
        vec![0.0, radius * std::f64::consts::FRAC_PI_2],
        Some(vec![[0.0, 0.0, -1.0], [1.0, 0.0, 0.0]]),
    )
}

pub(crate) fn close2(a: [f64; 2], b: [f64; 2], tolerance: f64) {
    for i in 0..2 {
        assert!((a[i] - b[i]).abs() <= tolerance, "{a:?} is not {b:?}");
    }
}

/// An axis over the whole path, knots and ends only.
pub(crate) fn axis_of(path: &PlacedPath) -> WellAxis {
    WellAxis::new(path, path.md_extent(), 0.0).expect("an axis")
}

pub(crate) const FRAME: &str = "f0000000-0000-4000-8000-000000000001";
pub(crate) const TRAJECTORY: &str = "70000000-0000-4000-8000-000000000001";
pub(crate) const CRS_UUID: &str = "c0000000-0000-4000-8000-000000000001";

/// `eml:///<qualified type>(<uuid>)`: the full uri a request carries.
pub(crate) fn uri(qualified_type: &str, uuid: &str) -> String {
    format!("eml:///{qualified_type}({uuid})")
}

pub(crate) fn frame_uri() -> String {
    uri("resqml22.WellboreFrameRepresentation", FRAME)
}

/// Lends the objects it holds, and reads no array from any file.
pub(crate) struct Package(pub(crate) Vec<Box<dyn DynObject>>);

impl ObjectSource for Package {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.0
            .iter()
            .find(|held| {
                energyml_core::identity::uuid_of_reflect(held.as_reflect())
                    .is_some_and(|held| held.eq_ignore_ascii_case(uuid))
            })
            .map(|held| held.clone_box())
    }
}

impl ArraySource for Package {
    fn locate(&self, _object: &dyn energyml_core::value::Reflect) -> Vec<energyml_storage::ArrayLocation> {
        Vec::new()
    }
    fn metadata(
        &self,
        location: &energyml_storage::ArrayLocation,
    ) -> energyml_domain::Result<energyml_storage::ArrayMetadata> {
        Err(energyml_domain::Error::Missing(location.field_path.clone()))
    }
    fn read(
        &self,
        location: &energyml_storage::ArrayLocation,
    ) -> energyml_domain::Result<energyml_storage::ArrayData> {
        Err(energyml_domain::Error::Missing(location.field_path.clone()))
    }
    fn read_into(
        &self,
        location: &energyml_storage::ArrayLocation,
        _out: &mut energyml_storage::array::ArrayBuffer<'_>,
    ) -> energyml_domain::Result<energyml_storage::ArrayMetadata> {
        Err(energyml_domain::Error::Missing(location.field_path.clone()))
    }
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
        uuid: CRS_UUID.to_string(),
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

/// A trajectory: the line (0, 0, 0) to (50, 0, 100) of the local CRS, over MD 0 to 100.
fn trajectory() -> resqml22::WellboreTrajectoryRepresentation {
    use resqml22::{
        AbstractParametricLineGeometry, AbstractPoint3dArray, ParametricLineGeometry, Point3d,
        Point3dLatticeArray, Point3dLatticeDimension,
    };
    resqml22::WellboreTrajectoryRepresentation {
        uuid: TRAJECTORY.to_string(),
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
                    uuid: CRS_UUID.to_string(),
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
                            coordinate1: 50.0,
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

/// A package holding a CRS, a trajectory and a frame whose nodes are at these depths.
pub(crate) fn package_with_well(node_md: &[f64]) -> Package {
    let frame = resqml22::WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
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
    };
    Package(vec![Box::new(depth_crs()), Box::new(trajectory()), Box::new(frame)])
}

/// What a request asks of the package of [`package_with_well`] and nothing more: the hole, no curtain.
pub(crate) fn hole_request(projection: &str) -> Request {
    Request {
        frame: frame_uri(),
        trajectory: None,
        curtain: None,
        boundaries: None,
        surfaces: Vec::new(),
        computed_tops: false,
        markers: Vec::new(),
        md_range: None,
        dn_range: None,
        projection: projection.to_string(),
        section: None,
        step: None,
    }
}

pub(crate) fn view() -> View {
    View {
        size: [800.0, 600.0],
        window: None,
        exaggeration: None,
        show_curtain: true,
        show_boundaries: true,
        show_surfaces: true,
        show_tops: true,
        colours: BTreeMap::new(),
        legend: None,
        embed_curtain: false,
    }
}

/// The section of an eastward vertical plane through the origin.
pub(crate) fn east_section() -> SectionAsk {
    SectionAsk {
        azimuth: std::f64::consts::FRAC_PI_2,
        origin: [0.0, 0.0],
    }
}

/// A model built by hand over a path: a curtain of `nodes` x `rows_up` with these values, and, when asked,
/// the boundaries of two rings (8 m above, 5 m below). Nothing is read.
pub(crate) fn model_on(
    path: PlacedPath,
    id: &str,
    nodes: &[f64],
    rows_up: &[f64],
    values: Vec<f64>,
    with_boundaries: bool,
) -> Model {
    let projections = WellProjections::standard();
    let section = east_section();
    let vertical_section = VerticalSection {
        azimuth: section.azimuth,
        origin: section.origin,
    };
    let projection: &dyn WellProjection = if id == "vertical_section" {
        &vertical_section
    } else {
        projections.get(id).expect("a projection of the registry")
    };
    let axis = axis_of(&path);
    let trace = axis
        .md
        .iter()
        .map(|&md| projection.place(&axis, md, 0.0).ok())
        .collect();
    let layout = CurtainLayout::new(nodes, rows_up, 0.0).expect("a layout");
    let geometry = curtain_cells(&axis, &layout, projection).expect("cells");
    let property = uri("resqml22.ContinuousProperty", "a0000000-0000-4000-8000-000000000001");
    let radius = uri("resqml22.ContinuousProperty", "a0000000-0000-4000-8000-000000000002");
    let angle = uri("resqml22.ContinuousProperty", "a0000000-0000-4000-8000-000000000003");
    let limits: Option<Boundaries> = with_boundaries.then(|| {
        let radii: Vec<f64> = nodes.iter().flat_map(|_| [8.0, 5.0]).collect();
        boundaries(
            &axis,
            nodes,
            &radii,
            &[0.0, std::f64::consts::PI],
            2,
            BoundaryMode::InPlane,
            projection,
        )
        .expect("boundaries")
    });
    let mut request = hole_request(id);
    request.section = (id == "vertical_section").then_some(section);
    request.curtain = Some(CurtainAsk {
        property: property.clone(),
        source: "cube-on-frame".to_string(),
        links: Vec::new(),
        roles: [
            (PlaneAxisAsk::HighSide, AxisSignAsk::Positive),
            (PlaneAxisAsk::Right, AxisSignAsk::Positive),
        ],
        lateral_index: 0,
    });
    if with_boundaries {
        request.boundaries = Some(BoundaryAsk {
            radius,
            angle,
            mode: BoundaryMode::InPlane,
        });
    }
    Model {
        request,
        path,
        axis,
        trace,
        node_md: nodes.to_vec(),
        curtain: Some(Curtain {
            layout,
            geometry,
            columns: CurtainColumns {
                property,
                values,
                rows_up: rows_up.to_vec(),
                lateral: 0.0,
                unit: None,
                notes: Vec::new(),
            },
        }),
        boundaries: limits,
        horizons: Vec::new(),
        tops: Vec::new(),
        markers: Vec::new(),
        markers_refused: Vec::new(),
        notes: Vec::new(),
    }
}

/// Every text of a drawing.
pub(crate) fn texts(list: &DisplayList) -> Vec<String> {
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
