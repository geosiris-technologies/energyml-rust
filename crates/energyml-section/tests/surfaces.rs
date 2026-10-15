//! `read::surfaces::read_surface` and `read::frame` on objects **fabricated after
//! the specification** (ADR-0011: the corpus is not authoritative): a
//! `TriangulatedSetRepresentation` is two triangles on a plane whose height is
//! known by formula, in a store held in memory, so every expected number is
//! arithmetic and not a measurement of somebody's file.
//!
//! # What it would take to make each test fail
//!
//! Said in each test (a test that cannot fail proves nothing).

use std::collections::HashMap;

use energyml_analysis::profile::{Corridor, Head};
use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect, Scalar};
use energyml_domain::reproject::{NoReprojection, Reprojector};
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_0 as eml20;
use energyml_resqml_2_0_1 as resqml20;
use energyml_section::model::{CutShape, Refused, SectionKey, SectionTrace, SurfaceRead, System, TraceRole};
use energyml_section::read::Scene;
use energyml_section::read::surfaces::{read_surface, uuid_of_uri};
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};

const CRS_ID: &str = "c0000000-0000-4000-8000-000000000001";
const TIME_CRS: &str = "c0000000-0000-4000-8000-000000000002";
const UTM_CRS: &str = "c0000000-0000-4000-8000-000000000003";
const FEET_CRS: &str = "c0000000-0000-4000-8000-000000000004";
const HORIZON: &str = "a0000000-0000-4000-8000-000000000001";
const FAULT: &str = "a0000000-0000-4000-8000-000000000002";
const PROXY: &str = "99999999-0000-4000-8000-000000000001";

// -- the store, held in memory ---------------------------------------------

/// An object the store holds, cloneable so it can be handed out boxed.
// A test fixture: a few objects, cloned out on demand; their sizes do not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Clone)]
enum Held {
    Surface(resqml20::TriangulatedSetRepresentation),
    Crs(resqml20::LocalDepth3dCrs),
    TimeCrs(resqml20::LocalTime3dCrs),
    Horizon(resqml20::HorizonInterpretation),
    Fault(resqml20::FaultInterpretation),
    Grid(resqml20::IjkGridRepresentation),
}

#[derive(Default)]
struct Store {
    objects: HashMap<String, Held>,
    datasets: HashMap<String, (ArrayValues, Vec<i64>)>,
    /// Arrays read - what a cache saves, counted where it is spent.
    reads: std::cell::Cell<usize>,
}

impl Store {
    fn put(&mut self, uuid: &str, held: Held) {
        self.objects.insert(uuid.to_string(), held);
    }
}

impl ObjectSource for Store {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        Some(match self.objects.get(uuid)?.clone() {
            Held::Surface(one) => Box::new(one),
            Held::Crs(one) => Box::new(one),
            Held::TimeCrs(one) => Box::new(one),
            Held::Horizon(one) => Box::new(one),
            Held::Fault(one) => Box::new(one),
            Held::Grid(one) => Box::new(one),
        })
    }
}

/// The dataset an array object points at: its `PathInHdfFile`, wherever it hangs.
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
                resource_uri: None,
            })
            .into_iter()
            .collect()
    }

    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        Ok(self.read(location)?.metadata)
    }

    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        self.reads.set(self.reads.get() + 1);
        let (values, dimensions) = self
            .datasets
            .get(&location.path_in_file)
            .ok_or_else(|| energyml_domain::Error::Missing(location.path_in_file.clone()))?;
        Ok(ArrayData {
            metadata: ArrayMetadata::new(dimensions.clone(), "f64".to_string()),
            values: values.clone(),
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

fn dor(uuid: &str, qualified: &str) -> eml20::DataObjectReference {
    eml20::DataObjectReference {
        uuid: uuid.to_string(),
        content_type: format!("application/x-resqml+xml;version=2.0;type={qualified}"),
        ..Default::default()
    }
}

fn dataset(path: &str) -> eml20::Hdf5Dataset {
    eml20::Hdf5Dataset {
        path_in_hdf_file: path.to_string(),
        hdf_proxy: dor(PROXY, "obj_EpcExternalPartReference"),
    }
}

fn local_crs(uuid: &str, epsg: u64, unit: eml20::LengthUom) -> resqml20::LocalDepth3dCrs {
    resqml20::LocalDepth3dCrs {
        uuid: uuid.to_string(),
        schema_version: "2.0".into(),
        projected_crs: Some(eml20::AbstractProjectedCrs::ProjectedCrsEpsgCode(
            eml20::ProjectedCrsEpsgCode { epsg_code: epsg },
        )),
        projected_uom: unit,
        vertical_uom: eml20::LengthUom::M,
        // An elevation: the third coordinate is the height, written as it stands.
        z_increasing_downward: false,
        ..Default::default()
    }
}

fn time_crs(uuid: &str, epsg: u64) -> resqml20::LocalTime3dCrs {
    resqml20::LocalTime3dCrs {
        uuid: uuid.to_string(),
        schema_version: "2.0".into(),
        projected_crs: Some(eml20::AbstractProjectedCrs::ProjectedCrsEpsgCode(
            eml20::ProjectedCrsEpsgCode { epsg_code: epsg },
        )),
        projected_uom: eml20::LengthUom::M,
        vertical_uom: eml20::LengthUom::M,
        time_uom: eml20::TimeUom::Ms,
        z_increasing_downward: true,
        ..Default::default()
    }
}

/// A triangulated surface in the store: its points and its triangles in
/// datasets, its geometry on `crs` (empty: it names none).
fn surface(
    store: &mut Store,
    uuid: &str,
    title: &str,
    crs: &str,
    interpretation: Option<(&str, &str)>,
    points: &[[f64; 3]],
    triangles: &[[u32; 3]],
) {
    let (points_path, triangles_path) = (format!("/{uuid}/points"), format!("/{uuid}/triangles"));
    store.datasets.insert(
        points_path.clone(),
        (
            ArrayValues::F64(points.iter().flatten().copied().collect()),
            vec![points.len() as i64, 3],
        ),
    );
    store.datasets.insert(
        triangles_path.clone(),
        (
            ArrayValues::I64(triangles.iter().flatten().map(|&i| i64::from(i)).collect()),
            vec![triangles.len() as i64, 3],
        ),
    );
    let local_crs = if crs.is_empty() {
        eml20::DataObjectReference::default()
    } else {
        dor(crs, "obj_LocalDepth3dCrs")
    };
    let patch = resqml20::TrianglePatch {
        patch_index: 0,
        count: triangles.len() as u64,
        node_count: points.len() as u64,
        triangles: (!triangles.is_empty()).then(|| {
            resqml20::AbstractIntegerArray::IntegerHdf5Array(resqml20::IntegerHdf5Array {
                null_value: -1,
                values: dataset(&triangles_path),
            })
        }),
        geometry: resqml20::AnyPointGeometry::PointGeometry(resqml20::PointGeometry {
            local_crs,
            points: Some(resqml20::AbstractPoint3dArray::Point3dHdf5Array(
                resqml20::Point3dHdf5Array {
                    coordinates: dataset(&points_path),
                },
            )),
            ..Default::default()
        }),
        ..Default::default()
    };
    store.put(
        uuid,
        Held::Surface(resqml20::TriangulatedSetRepresentation {
            uuid: uuid.to_string(),
            schema_version: "2.0".into(),
            citation: eml20::Citation {
                title: title.to_string(),
                ..Default::default()
            },
            represented_interpretation: interpretation.map(|(id, kind)| dor(id, kind)),
            triangle_patch: vec![patch],
            ..Default::default()
        }),
    );
}

/// The height of the fabricated plane, from the specification of the test and no
/// file: `z = 1000 + 0.2 y + 0.05 x`.
fn plane(x: f64, y: f64) -> f64 {
    1000.0 + 0.2 * y + 0.05 * x
}

/// Two triangles on that plane, over `(0, 0)..(1000, 1000)`, in the object's own
/// coordinates, every z computed by [`plane`].
fn plane_mesh() -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let corners = [[0.0, 0.0], [1000.0, 0.0], [1000.0, 1000.0], [0.0, 1000.0]];
    (
        corners.iter().map(|c| [c[0], c[1], plane(c[0], c[1])]).collect(),
        vec![[0, 1, 2], [0, 2, 3]],
    )
}

fn uri(uuid: &str) -> String {
    format!("eml:///resqml20.obj_TriangulatedSetRepresentation({uuid})")
}

/// The store the most tests use: the plane on `CRS_ID` (EPSG:2154, metres).
fn plane_store(uuid: &str, title: &str, interpretation: Option<(&str, &str)>) -> Store {
    let mut store = Store::default();
    store.put(CRS_ID, Held::Crs(local_crs(CRS_ID, 2154, eml20::LengthUom::M)));
    let (points, triangles) = plane_mesh();
    surface(
        &mut store,
        uuid,
        title,
        CRS_ID,
        interpretation,
        &points,
        &triangles,
    );
    store
}

fn lambert() -> System {
    System {
        epsg: 2154,
        unit: "m".into(),
    }
}

fn long(trace: &[[f64; 2]]) -> SectionTrace {
    SectionTrace {
        key: SectionKey::Long,
        trace: trace.to_vec(),
        left: 100.0,
        right: 100.0,
        crossing: None,
    }
}

#[allow(clippy::result_large_err)]
fn read_with(
    store: &Store,
    uuid: &str,
    sections: &[SectionTrace],
    system: Option<&System>,
    time_factor: Option<f64>,
    reprojector: &dyn Reprojector,
) -> Result<SurfaceRead, Refused> {
    let scene = Scene {
        system,
        time_factor,
        reprojector,
    };
    read_surface(store, "s1", &uri(uuid), sections, &scene)
}

#[allow(clippy::result_large_err)]
fn read(store: &Store, uuid: &str, sections: &[SectionTrace]) -> Result<SurfaceRead, Refused> {
    read_with(store, uuid, sections, Some(&lambert()), None, &NoReprojection)
}

fn spans_of(cut: &energyml_section::model::SurfaceCut) -> &Vec<Vec<[f64; 2]>> {
    match &cut.shape {
        CutShape::Trace { spans } => spans,
        other => panic!("a triangulated surface is cut as a trace, not {other:?}"),
    }
}

/// The surface's z at an abscissa, interpolated inside the span that holds it.
fn z_at(spans: &[Vec<[f64; 2]>], s: f64) -> f64 {
    for span in spans {
        for pair in span.windows(2) {
            if s >= pair[0][0] - 1e-9 && s <= pair[1][0] + 1e-9 {
                let width = pair[1][0] - pair[0][0];
                let t = if width < 1e-12 {
                    0.0
                } else {
                    (s - pair[0][0]) / width
                };
                return pair[0][1] + (pair[1][1] - pair[0][1]) * t;
            }
        }
    }
    panic!("no span holds s = {s}: {spans:?}")
}

// -- the tests -------------------------------------------------------------

/// A tilted plane cut by a straight trace is a straight line in `(s, z)`, and
/// the line is the one the formula gives: from `(0, plane(100, 500))` to
/// `(800, plane(900, 500))`, with slope 0.05. The object is named in full.
///
/// To make it fail: swap `x` and `y` in the trace (`[500, 100]`) and the line
/// moves; or have `cut_many` start the abscissa elsewhere than the trace's
/// first point.
#[test]
fn a_plane_cut_along_a_straight_trace_is_the_line_the_formula_gives() {
    let store = plane_store(HORIZON, "Top reservoir", None);
    let found = read(&store, HORIZON, &[long(&[[100.0, 500.0], [900.0, 500.0]])]).unwrap();

    assert_eq!(found.object.store, "s1");
    assert_eq!(found.object.uuid, HORIZON);
    assert_eq!(found.object.title, "Top reservoir");
    assert_eq!(
        found.object.qualified_type,
        "resqml20.obj_TriangulatedSetRepresentation"
    );
    assert!(!found.time);
    assert_eq!(found.cuts.len(), 1);
    assert_eq!(found.cuts[0].section, SectionKey::Long);
    assert!(found.cuts[0].notes.is_empty());

    let spans = spans_of(&found.cuts[0]);
    assert_eq!(
        spans.len(),
        1,
        "two triangles welded on their shared edge: one span"
    );
    let span = &spans[0];
    let (first, last) = (span.first().unwrap(), span.last().unwrap());
    assert!(
        first[0].abs() < 1e-9 && (last[0] - 800.0).abs() < 1e-9,
        "from s = 0 to s = 800: {span:?}"
    );
    assert!((first[1] - plane(100.0, 500.0)).abs() < 1e-9, "{first:?}");
    assert!((last[1] - plane(900.0, 500.0)).abs() < 1e-9, "{last:?}");
    for point in span {
        // The slope is 0.05 whatever the node: the line is straight.
        assert!(
            (point[1] - (plane(100.0, 500.0) + 0.05 * point[0])).abs() < 1e-9,
            "{point:?}"
        );
    }
    assert!(
        span.windows(2).all(|pair| pair[1][0] >= pair[0][0]),
        "s increases"
    );
}

/// A trace that does not cross the surface gives **no entry**: the surface is
/// read (`Ok`), it just crosses nothing - not a refusal, and not an empty cut. A
/// cross section that does cross has its own entry, under its own key.
///
/// To make it fail: push a `SurfaceCut` for every section whatever `found`
/// holds - the missed trace gets an empty one and `cuts.len()` is 2.
#[test]
fn a_trace_that_misses_the_surface_has_no_cut_and_the_one_that_crosses_keeps_its_key() {
    let store = plane_store(HORIZON, "Top", None);
    let missing = long(&[[5000.0, 5000.0], [6000.0, 5000.0]]);
    let found = read(&store, HORIZON, std::slice::from_ref(&missing)).unwrap();
    assert!(found.cuts.is_empty());

    let crossing = SectionTrace {
        key: SectionKey::Cross { index: 2 },
        ..long(&[[500.0, -100.0], [500.0, 1100.0]])
    };
    let found = read(&store, HORIZON, &[missing, crossing]).unwrap();
    assert_eq!(found.cuts.len(), 1);
    assert_eq!(found.cuts[0].section, SectionKey::Cross { index: 2 });
    let span = &spans_of(&found.cuts[0])[0];
    // Along y at x = 500, the surface is on [0, 1000] of the trace's 1200: the cut
    // starts 100 m in, at the height the formula gives, and rises 0.2 per metre.
    assert!((span[0][0] - 100.0).abs() < 1e-9, "{span:?}");
    assert!((span[0][1] - plane(500.0, 0.0)).abs() < 1e-9);
    assert!((span.last().unwrap()[1] - plane(500.0, 1000.0)).abs() < 1e-9);
}

/// **Triangulation** (ADR-0011: two independent sources of one fact).
/// The abscissa of a point of the cut and the one `Corridor::project` gives a
/// head set down at the same place agree - on a trace with a bend, where they
/// would part if either forgot to accumulate the length of the first leg.
///
/// The fact is the height at a given spot: `Corridor` knows nothing of surfaces
/// and says where, along the trace, a head at `(x, y)` falls (`s`, and its
/// `foot` on the trace); the cut says what the surface is at each `s`; the
/// formula says what it should be at that foot. Three heads: before the bend, at
/// the bend's outer wedge and well past it on the second leg, one of them set
/// to the side (its foot is not where the head is).
///
/// To make it fail: in `cut_many`, drop the `offset` of the segments - the
/// second leg is cut from s = 0 and `z_at` finds the first leg's heights there.
#[test]
fn the_abscissa_of_a_cut_is_the_one_corridor_gives_a_head_at_the_same_spot() {
    let store = plane_store(HORIZON, "Top", None);
    // East 400 m, then north 600 m: a right-angle bend at (500, 200).
    let trace = [[100.0, 200.0], [500.0, 200.0], [500.0, 800.0]];
    let found = read(&store, HORIZON, &[long(&trace)]).unwrap();
    let spans = spans_of(&found.cuts[0]);
    assert!(!spans.is_empty());

    let corridor = Corridor::new(&trace, 100.0, 100.0).unwrap();
    let heads = [
        Head::new("before the bend", [300.0, 200.0]),
        Head::new("past the bend", [500.0, 500.0]),
        Head::new("beside the second leg", [560.0, 650.0]),
    ];
    let profile = corridor.project(&heads);
    assert_eq!(profile.kept.len(), 3, "{profile:?}");
    // Not just "some s": the arc lengths the trace implies.
    assert!((profile.kept[0].s - 200.0).abs() < 1e-9);
    assert!((profile.kept[1].s - 700.0).abs() < 1e-9);
    assert!((profile.kept[2].s - 850.0).abs() < 1e-9);
    for kept in &profile.kept {
        let expected = plane(kept.foot[0], kept.foot[1]);
        let cut = z_at(spans, kept.s);
        assert!(
            (cut - expected).abs() < 1e-6,
            "{}: at s = {} the cut says z = {cut}, the surface at the head's foot {:?} says {expected}",
            kept.id,
            kept.s,
            kept.foot
        );
    }
}

/// Two systems. The object is in EPSG:32631 and the scene in EPSG:2154, and the
/// test's `Reprojector` is a **known translation** (+1000 east, +2000 north - no
/// geodesy, a fact the test writes down): the cut along a scene trace is then
/// the cut of the same plane along the same trace moved back by that vector,
/// which is the cut of the first test. And the call to the backend is real:
/// `NoReprojection` refuses the same read, naming the two codes.
///
/// To make it fail: skip the `reproject_placed` call in `to_scene` - the plane
/// sits 1000 m west of where it should and the trace misses it.
#[test]
fn an_object_in_another_system_is_cut_where_the_scene_draws_it() {
    struct Shift;
    impl Reprojector for Shift {
        fn reproject(&self, point: [f64; 3], from: u64, to: u64) -> energyml_domain::Result<[f64; 3]> {
            match (from, to) {
                (32631, 2154) => Ok([point[0] + 1000.0, point[1] + 2000.0, point[2]]),
                _ => Err(energyml_domain::Error::unsupported(
                    "the test's reprojection",
                    format!("{from} to {to}"),
                )),
            }
        }
    }

    let mut store = Store::default();
    store.put(UTM_CRS, Held::Crs(local_crs(UTM_CRS, 32631, eml20::LengthUom::M)));
    let (points, triangles) = plane_mesh();
    surface(&mut store, HORIZON, "Top", UTM_CRS, None, &points, &triangles);

    // The scene trace of the first test, moved by the vector.
    let trace = long(&[[1100.0, 2500.0], [1900.0, 2500.0]]);
    let moved = read_with(
        &store,
        HORIZON,
        std::slice::from_ref(&trace),
        Some(&lambert()),
        None,
        &Shift,
    )
    .unwrap();
    let reference = read(
        &plane_store(HORIZON, "Top", None),
        HORIZON,
        &[long(&[[100.0, 500.0], [900.0, 500.0]])],
    )
    .unwrap();
    assert_eq!(
        spans_of(&moved.cuts[0]),
        spans_of(&reference.cuts[0]),
        "the same line in (s, z)"
    );

    let refused = read_with(&store, HORIZON, &[trace], Some(&lambert()), None, &NoReprojection).unwrap_err();
    assert!(
        refused.reason.contains("32631") && refused.reason.contains("2154"),
        "{}",
        refused.reason
    );
}

/// The same EPSG code in another unit is a conversion, not an identity: a plane
/// written in feet is cut along a trace in metres at `0.3048` of its numbers -
/// and the backend is not asked (`NoReprojection` would refuse).
///
/// To make it fail: have `frame_of` read the unit as `"m"` whatever the CRS
/// says - the plane is 3.28 times too big and the line ends at s = 800 / 0.3048.
#[test]
fn the_same_system_in_feet_is_converted_to_the_scenes_metres() {
    let mut store = Store::default();
    store.put(
        FEET_CRS,
        Held::Crs(local_crs(FEET_CRS, 2154, eml20::LengthUom::Ft)),
    );
    let (points, triangles) = plane_mesh();
    surface(&mut store, HORIZON, "Top", FEET_CRS, None, &points, &triangles);

    // 1000 ft = 304.8 m: a trace along y = 150 m from x = 30 m to x = 270 m.
    let found = read(&store, HORIZON, &[long(&[[30.0, 150.0], [270.0, 150.0]])]).unwrap();
    let span = &spans_of(&found.cuts[0])[0];
    assert!(
        (span.last().unwrap()[0] - 240.0).abs() < 1e-6,
        "the whole trace is on the plane: {span:?}"
    );
    // Horizontal numbers in metres, z as the file says it (the vertical is not
    // the horizontal's to convert): the height at x = 30 m is that of x = 30 / 0.3048 ft.
    let expected = plane(30.0 / 0.3048, 150.0 / 0.3048);
    assert!(
        (span[0][1] - expected).abs() < 1e-6,
        "{} vs {expected}",
        span[0][1]
    );
}

/// A system the object names and the scene does not (and the reverse) is
/// refused **by name**, never drawn on coordinates it does not share; an object
/// that names no CRS at all is in the second case.
///
/// To make it fail: let `(Some, None)` or `(None, Some)` fall through in
/// `to_scene` with the points unchanged - each read then succeeds.
#[test]
fn a_system_on_one_side_only_is_refused_by_name() {
    let store = plane_store(HORIZON, "Top", None);
    let trace = [long(&[[100.0, 500.0], [900.0, 500.0]])];

    let refused = read_with(&store, HORIZON, &trace, None, None, &NoReprojection).unwrap_err();
    assert!(
        refused.reason.contains("EPSG:2154") && refused.reason.contains("scene names no system"),
        "{}",
        refused.reason
    );
    assert_eq!(refused.object.title, "Top", "the refusal names the object");

    let mut bare = Store::default();
    let (points, triangles) = plane_mesh();
    surface(&mut bare, FAULT, "No CRS", "", None, &points, &triangles);
    let refused = read_with(&bare, FAULT, &trace, Some(&lambert()), None, &NoReprojection).unwrap_err();
    assert!(
        refused.reason.contains("names no horizontal system"),
        "{}",
        refused.reason
    );
    // Neither side names one: the local numbers are the only ones there are.
    let found = read_with(&bare, FAULT, &trace, None, None, &NoReprojection).unwrap();
    assert_eq!(found.cuts.len(), 1);
}

/// An object in time is carried onto the depth axis by the scene's factor and
/// marked `time`, and **refused by name without one**: a seismic horizon at
/// 2 000 ms drawn at 2 000 m is a plausible picture of the wrong place. A depth
/// object does not take the factor.
///
/// To make it fail: drop the `time_factor` multiplication in `to_scene` (z stays
/// 2 000), or let a missing factor pass as 1 (the refusal vanishes), or apply
/// the factor to every object (the depth plane in the last assertion moves).
#[test]
fn an_object_in_time_takes_the_scenes_time_factor_or_is_refused() {
    let flat = |z: f64| -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
        (
            vec![
                [0.0, 0.0, z],
                [1000.0, 0.0, z],
                [1000.0, 1000.0, z],
                [0.0, 1000.0, z],
            ],
            vec![[0, 1, 2], [0, 2, 3]],
        )
    };
    let trace = [long(&[[100.0, 500.0], [900.0, 500.0]])];
    let mut store = Store::default();
    store.put(TIME_CRS, Held::TimeCrs(time_crs(TIME_CRS, 2154)));
    store.put(CRS_ID, Held::Crs(local_crs(CRS_ID, 2154, eml20::LengthUom::M)));
    let (points, triangles) = flat(2000.0);
    surface(
        &mut store,
        HORIZON,
        "Seismic top",
        TIME_CRS,
        None,
        &points,
        &triangles,
    );
    let (points, triangles) = flat(2000.0);
    surface(&mut store, FAULT, "Depth top", CRS_ID, None, &points, &triangles);

    let refused = read_with(&store, HORIZON, &trace, Some(&lambert()), None, &NoReprojection).unwrap_err();
    assert!(refused.reason.contains("time factor"), "{}", refused.reason);
    assert_eq!(refused.object.title, "Seismic top");

    let found = read_with(
        &store,
        HORIZON,
        &trace,
        Some(&lambert()),
        Some(0.75),
        &NoReprojection,
    )
    .unwrap();
    assert!(found.time);
    for point in &spans_of(&found.cuts[0])[0] {
        // The CRS says z increases downward, so the placement turns 2 000 into
        // an elevation of -2 000 first, and the factor scales that.
        assert!(
            (point[1] + 1500.0).abs() < 1e-9,
            "-2 000 ms at 0.75 m per ms: {point:?}"
        );
    }
    let depth = read_with(
        &store,
        FAULT,
        &trace,
        Some(&lambert()),
        Some(0.75),
        &NoReprojection,
    )
    .unwrap();
    assert!(!depth.time);
    assert!(
        spans_of(&depth.cuts[0])[0]
            .iter()
            .all(|p| (p[1] - 2000.0).abs() < 1e-9),
        "a depth is not scaled"
    );
}

/// What cannot be cut is refused by name: a grid the reader cannot read (its
/// envelope is tested in `tests/grids.rs`), an object the package does not
/// hold, one that is not a shape (a CRS), a surface with no triangle at all.
/// **Never an empty answer.**
///
/// To make it fail: return `Ok` with no cuts for the object that is not a shape.
#[test]
fn what_cannot_be_cut_is_refused_by_name() {
    let mut store = plane_store(HORIZON, "Top", None);
    store.put(
        FAULT,
        Held::Grid(resqml20::IjkGridRepresentation {
            uuid: FAULT.to_string(),
            citation: eml20::Citation {
                title: "Reservoir grid".into(),
                ..Default::default()
            },
            ..Default::default()
        }),
    );
    let (points, _) = plane_mesh();
    surface(
        &mut store,
        "e0000000-0000-4000-8000-000000000001",
        "Triangle-less",
        CRS_ID,
        None,
        &points,
        &[],
    );
    let trace = [long(&[[100.0, 500.0], [900.0, 500.0]])];

    let grid = read(&store, FAULT, &trace).unwrap_err();
    // An IJK grid has its own reading: this one states no size, so it
    // is refused by the grid reader, naming the grid.
    assert!(
        grid.reason.contains("Reservoir grid") && grid.reason.contains("IjkGridRepresentation"),
        "{}",
        grid.reason
    );
    assert_eq!(grid.object.title, "Reservoir grid");
    assert_eq!(grid.object.qualified_type, "resqml20.obj_IjkGridRepresentation");

    let crs = read(&store, CRS_ID, &trace).unwrap_err();
    assert!(crs.reason.contains("LocalDepth3dCrs"), "{}", crs.reason);

    let none = read(&store, "e0000000-0000-4000-8000-000000000001", &trace).unwrap_err();
    assert!(none.reason.contains("no triangles"), "{}", none.reason);

    let absent = read(&store, "f0000000-0000-4000-8000-0000000000ff", &trace).unwrap_err();
    assert!(
        absent.reason.contains("cannot be read from the package"),
        "{}",
        absent.reason
    );
    assert_eq!(absent.object.uri, uri("f0000000-0000-4000-8000-0000000000ff"));
}

/// The role is the **type of the interpretation**, never the title: a surface
/// titled "Fault_3" that represents a `HorizonInterpretation` is a horizon, one
/// titled "Top" that represents a `FaultInterpretation` is a fault, and one that
/// represents nothing is `Other`.
///
/// To make it fail: decide the role from `object.title` (the first two are then
/// swapped), or from the qualified type of the reference only when the store
/// holds the interpretation (it does here, and carries a different spelling).
#[test]
fn the_role_comes_from_the_interpretation_and_never_from_the_title() {
    let mut store = plane_store(
        HORIZON,
        "Fault_3",
        Some((
            "b0000000-0000-4000-8000-000000000001",
            "obj_HorizonInterpretation",
        )),
    );
    store.put(
        "b0000000-0000-4000-8000-000000000001",
        Held::Horizon(resqml20::HorizonInterpretation {
            uuid: "b0000000-0000-4000-8000-000000000001".into(),
            ..Default::default()
        }),
    );
    let (points, triangles) = plane_mesh();
    surface(
        &mut store,
        FAULT,
        "Top",
        CRS_ID,
        Some(("b0000000-0000-4000-8000-000000000002", "obj_FaultInterpretation")),
        &points,
        &triangles,
    );
    store.put(
        "b0000000-0000-4000-8000-000000000002",
        Held::Fault(resqml20::FaultInterpretation {
            uuid: "b0000000-0000-4000-8000-000000000002".into(),
            ..Default::default()
        }),
    );
    // A third that names an interpretation the package does not hold: the
    // reference's own type is the fallback.
    surface(
        &mut store,
        "d0000000-0000-4000-8000-000000000003",
        "Horizon_1",
        CRS_ID,
        Some(("b0000000-0000-4000-8000-0000000000aa", "obj_FaultInterpretation")),
        &points,
        &triangles,
    );
    surface(
        &mut store,
        "d0000000-0000-4000-8000-000000000004",
        "Horizon_2",
        CRS_ID,
        None,
        &points,
        &triangles,
    );
    let trace = [long(&[[100.0, 500.0], [900.0, 500.0]])];

    assert_eq!(read(&store, HORIZON, &trace).unwrap().role, TraceRole::Horizon);
    assert_eq!(read(&store, FAULT, &trace).unwrap().role, TraceRole::Fault);
    assert_eq!(
        read(&store, "d0000000-0000-4000-8000-000000000003", &trace)
            .unwrap()
            .role,
        TraceRole::Fault
    );
    assert_eq!(
        read(&store, "d0000000-0000-4000-8000-000000000004", &trace)
            .unwrap()
            .role,
        TraceRole::Other
    );
}

/// A source that holds the interpretation of [`the_role_comes_from_the_version_the_reference_names`]
/// in **two versions**: asked a uuid it gives the first (what the inner store
/// holds), and only a uri naming `version='2'` gives the second.
struct TwoVersions<'a> {
    inner: &'a Store,
    interpretation: &'a str,
    second: resqml20::FaultInterpretation,
}

impl ObjectSource for TwoVersions<'_> {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        self.inner.object(uuid)
    }

    fn object_at(&self, uri: &str) -> Option<Box<dyn DynObject>> {
        let parsed = energyml_core::identifiers::EtpUri::parse(uri).ok();
        match parsed {
            Some(parsed)
                if parsed.uuid.as_deref() == Some(self.interpretation)
                    && parsed.object_version.as_deref() == Some("2") =>
            {
                Some(Box::new(self.second.clone()))
            }
            _ => self.inner.object_at(uri),
        }
    }
}

impl ArraySource for TwoVersions<'_> {
    fn locate(&self, object: &dyn Reflect) -> Vec<ArrayLocation> {
        self.inner.locate(object)
    }
    fn metadata(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        self.inner.metadata(location)
    }
    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        self.inner.read(location)
    }
    fn read_into(
        &self,
        location: &ArrayLocation,
        out: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        self.inner.read_into(location, out)
    }
}

/// An interpretation held in two versions: the surface's reference names
/// **version 2**, which is a fault; version 1 (the one a uuid alone reaches) is
/// a horizon. The role is the one of the version named.
///
/// Internally only the full uri is used (ADR-0003) - a package can hold one uuid twice.
///
/// To make it fail: in `surfaces::role_of` (or `scene::interpretation_of`) ask
/// the source for the interpretation's uuid - the role is then `Horizon`.
#[test]
fn the_role_comes_from_the_version_the_reference_names() {
    let interpretation = "b0000000-0000-4000-8000-000000000001";
    let mut store = plane_store(HORIZON, "Top", Some((interpretation, "obj_FaultInterpretation")));
    store.put(
        interpretation,
        Held::Horizon(resqml20::HorizonInterpretation {
            uuid: interpretation.into(),
            ..Default::default()
        }),
    );
    let Some(Held::Surface(held)) = store.objects.get_mut(HORIZON) else {
        panic!("the surface")
    };
    held.represented_interpretation
        .as_mut()
        .expect("its interpretation")
        .version_string = Some("2".into());

    let second = resqml20::FaultInterpretation {
        uuid: interpretation.into(),
        ..Default::default()
    };
    let source = TwoVersions {
        inner: &store,
        interpretation,
        second,
    };
    let trace = [long(&[[100.0, 500.0], [900.0, 500.0]])];
    let system = lambert();
    let scene = Scene {
        system: Some(&system),
        time_factor: None,
        reprojector: &NoReprojection,
    };
    let found = read_surface(&source, "s1", &uri(HORIZON), &trace, &scene).expect("reads");
    assert_eq!(
        found.role,
        TraceRole::Fault,
        "version 2 is a fault; the uuid alone would have given a horizon"
    );
}

/// A uri names its object by the uuid in its parentheses; a bare uuid is taken
/// as itself, lower-cased - the store keys its objects that way.
///
/// To make it fail: return the uri untouched.
#[test]
fn a_uri_gives_its_uuid_in_lower_case() {
    assert_eq!(
        uuid_of_uri(
            "eml:///resqml20.obj_TriangulatedSetRepresentation(A0000000-0000-4000-8000-000000000001)"
        ),
        HORIZON
    );
    assert_eq!(
        uuid_of_uri(
            "eml:///dataspace('x')/resqml22.TriangulatedSetRepresentation(a0000000-0000-4000-8000-000000000001)"
        ),
        HORIZON
    );
    assert_eq!(uuid_of_uri("A0000000-0000-4000-8000-000000000001"), HORIZON);
}

#[test]
fn a_resolver_the_face_passes_answers_instead_of_the_cascade() {
    use energyml_domain::color::GraphicalSet;
    use energyml_section::read::surfaces::{Colouring, read_surface_coloured};

    let store = plane_store(HORIZON, "Top", None);
    let trace = [long(&[[100.0, 500.0], [900.0, 500.0]])];
    let system = lambert();
    let scene = Scene {
        system: Some(&system),
        time_factor: None,
        reprojector: &NoReprojection,
    };
    let asked = std::cell::RefCell::new(Vec::new());
    let resolver = |uri: &str| -> Option<String> {
        asked.borrow_mut().push(uri.to_string());
        Some("#010203".to_string())
    };
    // A set is present too: given a resolver, it is never read.
    let set = GraphicalSet::default();
    let colouring = Colouring {
        set: Some(&set),
        index: None,
        resolver: Some(&resolver),
    };
    let read = read_surface_coloured(&store, "s1", &uri(HORIZON), &trace, &scene, colouring).expect("reads");
    assert_eq!(read.colour.as_deref(), Some("#010203"));
    // By the full uri: two versions of a surface are two surfaces (identity).
    assert_eq!(
        *asked.borrow(),
        vec![uri(HORIZON)],
        "asked once, by the object's uri"
    );

    // No resolver and an empty set: the file says nothing, and nothing is made up.
    let quiet = Colouring {
        set: Some(&set),
        index: None,
        resolver: None,
    };
    let read = read_surface_coloured(&store, "s1", &uri(HORIZON), &trace, &scene, quiet).expect("reads");
    assert_eq!(read.colour, None);
}

// -- a package's cache, and several packages ----------------------------------

fn asked(store: &str, revision: u64, surfaces: &[&str]) -> energyml_section::model::Asked {
    energyml_section::model::Asked {
        store: store.to_string(),
        revision,
        wells: Vec::new(),
        surfaces: surfaces.iter().map(|uuid| uri(uuid)).collect(),
    }
}

/// `read::cache`: a surface is read and placed once per revision of its
/// package; moving the trace cuts it again **without reading it**; a second
/// run on the same trace does nothing at all. Counted on the store's own array
/// reads - the work the cache exists to save - and the cached answer is the
/// uncached one.
///
/// To make it fail: key the placed surface on the sections too (the moved
/// trace reads again), or forget the revision (the edit is not seen).
#[test]
fn a_package_cache_reads_each_surface_once_and_cuts_again_only_when_the_trace_moves() {
    use energyml_section::read::{PackageCache, PackageContext, Work, contribution, contribution_cached};

    let store = plane_store(HORIZON, "Top", None);
    let system = lambert();
    let scene = Scene {
        system: Some(&system),
        time_factor: None,
        reprojector: &NoReprojection,
    };
    let context = PackageContext::default();
    let east = [long(&[[100.0, 500.0], [900.0, 500.0]])];
    let north = [long(&[[500.0, 100.0], [500.0, 900.0]])];
    let mut cache = PackageCache::new();

    let first = contribution_cached(
        &store,
        &asked("s1", 0, &[HORIZON]),
        &east,
        &scene,
        &context,
        &mut cache,
    );
    let reads = store.reads.get();
    assert!(reads > 0, "the first run reads the surface");
    assert_eq!(
        first,
        contribution(&store, &asked("s1", 0, &[HORIZON]), &east, &scene, &context),
        "a cached answer is the answer"
    );
    let reads_uncached = store.reads.get() - reads;

    let again = contribution_cached(
        &store,
        &asked("s1", 0, &[HORIZON]),
        &east,
        &scene,
        &context,
        &mut cache,
    );
    assert_eq!(again, first);
    assert_eq!(
        cache.work(),
        Work {
            wells_read: 0,
            surfaces_read: 1,
            surfaces_cut: 1,
            sections_cut: 1,
            reused: 1,
            ..Work::default()
        }
    );

    let moved = contribution_cached(
        &store,
        &asked("s1", 0, &[HORIZON]),
        &north,
        &scene,
        &context,
        &mut cache,
    );
    assert_eq!(
        store.reads.get(),
        reads + reads_uncached,
        "moving the trace reads nothing"
    );
    assert_eq!(cache.work().surfaces_cut, 2);
    assert_ne!(moved, first, "and it cuts along the new trace");

    contribution_cached(
        &store,
        &asked("s1", 1, &[HORIZON]),
        &north,
        &scene,
        &context,
        &mut cache,
    );
    assert_eq!(
        cache.work().surfaces_read,
        2,
        "another revision of the package is read again"
    );
}

/// `read::cache`, lot 6a: a cut is kept under (surface, **line**), several per
/// surface. A section added cuts only its own line; a corridor, a key or the
/// order of the sections is not a line; and what is served is what a cache that
/// never kept anything gives, each cut labelled with the key of the section
/// asking.
///
/// To make it fail: key the cuts on the JSON of every section again (the added
/// section re-cuts the first, and the width cuts again), or serve a kept cut
/// without relabelling it (the swapped keys come back as they were cut).
#[test]
fn a_cut_is_kept_for_its_line_whoever_asks() {
    use energyml_section::read::{PackageCache, PackageContext, contribution, contribution_cached};

    let store = plane_store(HORIZON, "Top", None);
    let system = lambert();
    let scene = Scene {
        system: Some(&system),
        time_factor: None,
        reprojector: &NoReprojection,
    };
    let context = PackageContext::default();
    let east = long(&[[100.0, 500.0], [900.0, 500.0]]);
    let north = SectionTrace {
        key: SectionKey::Cross { index: 0 },
        ..long(&[[500.0, 100.0], [500.0, 900.0]])
    };
    let mut cache = PackageCache::new();
    let run = |sections: &[SectionTrace], cache: &mut PackageCache| {
        contribution_cached(
            &store,
            &asked("s1", 0, &[HORIZON]),
            sections,
            &scene,
            &context,
            cache,
        )
    };

    run(std::slice::from_ref(&east), &mut cache);
    assert_eq!((cache.work().surfaces_cut, cache.work().sections_cut), (1, 1));

    // A second section: only its line is cut.
    let both = run(&[east.clone(), north.clone()], &mut cache);
    assert_eq!((cache.work().surfaces_cut, cache.work().sections_cut), (2, 2));
    assert_eq!(
        both,
        contribution(
            &store,
            &asked("s1", 0, &[HORIZON]),
            &[east.clone(), north.clone()],
            &scene,
            &context
        )
    );

    // A corridor is not a line.
    let wider = SectionTrace {
        left: 250.0,
        right: 30.0,
        ..east.clone()
    };
    run(&[wider, north.clone()], &mut cache);
    assert_eq!(
        (cache.work().surfaces_cut, cache.work().sections_cut),
        (2, 2),
        "nothing cut for a width"
    );

    // The same two lines under the other keys, in the other order: nothing cut, and each cut carries the key of the section that asked.
    let swapped = [
        SectionTrace {
            key: SectionKey::Cross { index: 0 },
            ..east.clone()
        },
        SectionTrace {
            key: SectionKey::Long,
            ..north.clone()
        },
    ];
    let served = run(&swapped, &mut cache);
    assert_eq!(
        (cache.work().surfaces_cut, cache.work().sections_cut),
        (2, 2),
        "keys and order are not lines"
    );
    assert_eq!(
        served,
        contribution(&store, &asked("s1", 0, &[HORIZON]), &swapped, &scene, &context)
    );
    assert!(
        !served.surfaces[0].cuts.is_empty(),
        "the horizon is cut by the lines (else the equality above proves nothing)"
    );
}

/// `compute_packages`: two packages, each with its own selection, give **one**
/// catalogue - both surfaces cut along the same trace, each under its
/// package's id - and a selection naming a package that is not open is
/// refused object by object, naming the package.
///
/// To make it fail: read every selection from the first package (the second
/// surface is not in it and comes back refused), or drop the unopened one.
#[test]
fn two_packages_merge_into_one_catalogue_and_an_unopened_one_is_refused_by_name() {
    use energyml_section::model::{Crosses, Inputs, Outcome};
    use energyml_section::read::{PackageCache, PackageContext};
    use energyml_section::{Opened, compute_packages};

    let first = plane_store(HORIZON, "Top", None);
    let second = plane_store(FAULT, "Base", None);
    let inputs = Inputs {
        trace: vec![[100.0, 500.0], [900.0, 500.0]],
        system: Some(lambert()),
        left: 100.0,
        right: 100.0,
        crosses: Crosses::None,
        objects: vec![
            asked("s1", 0, &[HORIZON]),
            asked("s2", 0, &[FAULT]),
            asked("s3", 0, &[PROXY]),
        ],
        time_factor: None,
    };
    let mut kept = PackageCache::new();
    let mut opened = [
        Opened {
            store: "s1".into(),
            source: &first,
            context: PackageContext::default(),
            cache: Some(&mut kept),
        },
        Opened {
            store: "s2".into(),
            source: &second,
            context: PackageContext::default(),
            cache: None,
        },
    ];
    let catalogue = compute_packages(&inputs, &mut opened, &NoReprojection).expect("the inputs are valid");

    let row = |store: &str| {
        catalogue
            .objects
            .iter()
            .find(|row| row.object.store == store)
            .unwrap_or_else(|| panic!("no row from {store}"))
    };
    assert!(
        matches!(row("s1").outcome, Outcome::Drawn { .. }),
        "{:?}",
        row("s1").outcome
    );
    assert!(
        matches!(row("s2").outcome, Outcome::Drawn { .. }),
        "{:?}",
        row("s2").outcome
    );
    assert_eq!(row("s2").object.title, "Base");
    match &row("s3").outcome {
        Outcome::Refused { reason } => {
            assert!(reason.contains("`s3`") && reason.contains("not open"), "{reason}")
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(kept.work().surfaces_read, 1, "the first package's cache was used");
}
