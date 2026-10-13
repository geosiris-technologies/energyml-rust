//! The envelope of an IJK grid along a section — on grids **fabricated
//! in the test after the specification** (ADR-0011: the corpus is not
//! authoritative): pillars on a lattice, levels at known heights, split coordinate
//! lines written as RESQML writes them. Every expected number is arithmetic on
//! what the fixture states, never a measurement of somebody's file.
//!
//! The rules quoted are the ones the reading leans on: a corner is
//! `pillar` or, on the far side of a fault, the **split coordinate line**
//! (RESQML TRG 2.0.1, `ColumnLayerSplitCoordinateLines`); a K gap adds one level
//! of points (`KGaps`: *"NKL = NK + gapCount + 1"*); a cell the
//! `CellGeometryIsDefined` flag marks undefined has no shape (TRG §5.1).
//!
//! Each test says what would make it fail (a test that cannot fail
//! proves nothing).

use std::collections::HashMap;

use energyml_core::registry::DynObject;
use energyml_core::value::{FieldValue, Reflect, Scalar};
use energyml_domain::reproject::NoReprojection;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_0 as eml20;
use energyml_resqml_2_0_1 as resqml20;
use energyml_section::model::{CutShape, Refused, SectionKey, SectionTrace, SurfaceCut, SurfaceRead, System};
use energyml_section::read::Scene;
use energyml_section::read::surfaces::read_surface;
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata, ArrayValues};

const CRS_ID: &str = "c0000000-0000-4000-8000-000000000001";
const TIME_CRS: &str = "c0000000-0000-4000-8000-000000000002";
const GRID: &str = "d0000000-0000-4000-8000-000000000001";
const PROXY: &str = "99999999-0000-4000-8000-000000000001";

type Spans = Vec<Vec<[f64; 2]>>;

// -- the store, held in memory ---------------------------------------------

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
enum Held {
    Crs(resqml20::LocalDepth3dCrs),
    TimeCrs(resqml20::LocalTime3dCrs),
    Grid(Box<resqml20::IjkGridRepresentation>),
}

#[derive(Default)]
struct Store {
    objects: HashMap<String, Held>,
    datasets: HashMap<String, (ArrayValues, Vec<i64>)>,
}

impl ObjectSource for Store {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        Some(match self.objects.get(uuid)?.clone() {
            Held::Crs(one) => Box::new(one),
            Held::TimeCrs(one) => Box::new(one),
            Held::Grid(one) => Box::new(*one),
        })
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
        Ok(self.read(location)?.metadata)
    }

    fn read(&self, location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        let (values, dimensions) = self
            .datasets
            .get(&location.path_in_file)
            .ok_or_else(|| energyml_domain::Error::Missing(location.path_in_file.clone()))?;
        Ok(ArrayData {
            metadata: ArrayMetadata {
                dimensions: dimensions.clone(),
                element_type: "f64".to_string(),
            },
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

// -- grids, fabricated -----------------------------------------------------

/// What a grid is made of, as the standard states it.
#[derive(Default)]
struct Spec {
    /// `Ni`, `Nj`, `Nk`.
    size: [u64; 3],
    /// `Points`, in the order `(level, line)` with the line varying fastest.
    points: Vec<[f64; 3]>,
    /// `CellGeometryIsDefined`, in RESQML's cell order (i, then j, then k).
    defined: Option<Vec<bool>>,
    /// Split coordinate lines: the pillar each duplicates, and the columns that use it.
    splits: Vec<(i64, Vec<i64>)>,
    /// `GapAfterLayer`, of length `Nk - 1`.
    gaps: Option<Vec<bool>>,
    /// The CRS the geometry names.
    crs: &'static str,
}

fn ints(store: &mut Store, path: &str, values: &[i64]) -> eml20::Hdf5Dataset {
    store.datasets.insert(
        path.to_string(),
        (ArrayValues::I64(values.to_vec()), vec![values.len() as i64]),
    );
    dataset(path)
}

fn bools(store: &mut Store, path: &str, values: &[bool]) -> resqml20::AbstractBooleanArray {
    store.datasets.insert(
        path.to_string(),
        (
            ArrayValues::U8(values.iter().map(|v| u8::from(*v)).collect()),
            vec![values.len() as i64],
        ),
    );
    resqml20::AbstractBooleanArray::BooleanHdf5Array(resqml20::BooleanHdf5Array {
        values: dataset(path),
    })
}

fn put_grid(store: &mut Store, title: &str, spec: &Spec) {
    let path = format!("/{GRID}/points");
    store.datasets.insert(
        path.clone(),
        (
            ArrayValues::F64(spec.points.iter().flatten().copied().collect()),
            vec![spec.points.len() as i64, 3],
        ),
    );
    let split_coordinate_lines = (!spec.splits.is_empty()).then(|| {
        let pillars: Vec<i64> = spec.splits.iter().map(|s| s.0).collect();
        let elements: Vec<i64> = spec.splits.iter().flat_map(|s| s.1.clone()).collect();
        let mut total = 0;
        let cumulative: Vec<i64> = spec
            .splits
            .iter()
            .map(|s| {
                total += s.1.len() as i64;
                total
            })
            .collect();
        let integer = |values: eml20::Hdf5Dataset| {
            resqml20::AbstractIntegerArray::IntegerHdf5Array(resqml20::IntegerHdf5Array {
                null_value: -1,
                values,
            })
        };
        let pillar_values = ints(store, "/split/pillars", &pillars);
        let element_values = ints(store, "/split/elements", &elements);
        let cumulative_values = ints(store, "/split/cumulative", &cumulative);
        resqml20::ColumnLayerSplitCoordinateLines {
            count: spec.splits.len() as u64,
            pillar_indices: Some(integer(pillar_values)),
            columns_per_split_coordinate_line: resqml20::ResqmlJaggedArray {
                elements: Some(resqml20::AbstractValueArray::IntegerHdf5Array(
                    resqml20::IntegerHdf5Array {
                        null_value: -1,
                        values: element_values,
                    },
                )),
                cumulative_length: Some(integer(cumulative_values)),
            },
            ..Default::default()
        }
    });
    let cell_geometry_is_defined = spec.defined.as_ref().map(|flags| bools(store, "/defined", flags));
    let k_gaps = spec.gaps.as_ref().map(|flags| resqml20::KGaps {
        count: Some(flags.iter().filter(|g| **g).count() as u64),
        gap_after_layer: Some(bools(store, "/gaps", flags)),
    });
    let crs_kind = if spec.crs == TIME_CRS {
        "obj_LocalTime3dCrs"
    } else {
        "obj_LocalDepth3dCrs"
    };
    store.objects.insert(
        GRID.to_string(),
        Held::Grid(Box::new(resqml20::IjkGridRepresentation {
            uuid: GRID.to_string(),
            schema_version: "2.0".into(),
            citation: eml20::Citation {
                title: title.to_string(),
                ..Default::default()
            },
            ni: spec.size[0],
            nj: spec.size[1],
            nk: spec.size[2],
            k_gaps,
            geometry: Some(resqml20::IjkGridGeometry {
                local_crs: dor(spec.crs, crs_kind),
                points: Some(resqml20::AbstractPoint3dArray::Point3dHdf5Array(
                    resqml20::Point3dHdf5Array {
                        coordinates: dataset(&path),
                    },
                )),
                grid_is_righthanded: true,
                cell_geometry_is_defined,
                split_coordinate_lines,
                ..Default::default()
            }),
            ..Default::default()
        })),
    );
}

fn depth_crs() -> resqml20::LocalDepth3dCrs {
    resqml20::LocalDepth3dCrs {
        uuid: CRS_ID.to_string(),
        schema_version: "2.0".into(),
        projected_crs: Some(eml20::AbstractProjectedCrs::ProjectedCrsEpsgCode(
            eml20::ProjectedCrsEpsgCode { epsg_code: 2154 },
        )),
        projected_uom: eml20::LengthUom::M,
        vertical_uom: eml20::LengthUom::M,
        // An elevation: z is written as it stands.
        z_increasing_downward: false,
        ..Default::default()
    }
}

fn time_crs() -> resqml20::LocalTime3dCrs {
    resqml20::LocalTime3dCrs {
        uuid: TIME_CRS.to_string(),
        schema_version: "2.0".into(),
        projected_crs: Some(eml20::AbstractProjectedCrs::ProjectedCrsEpsgCode(
            eml20::ProjectedCrsEpsgCode { epsg_code: 2154 },
        )),
        projected_uom: eml20::LengthUom::M,
        vertical_uom: eml20::LengthUom::M,
        time_uom: eml20::TimeUom::Ms,
        z_increasing_downward: true,
        ..Default::default()
    }
}

/// A grid of vertical pillars on a lattice of `dx` by `dy`, every level at
/// `z[level]`; `lines` are the pillars only unless `extra` adds lines after
/// them (split coordinate lines, each given by its `(x, y)` and its own heights).
fn lattice(ni: usize, nj: usize, dx: f64, dy: f64, z: &[f64]) -> Vec<[f64; 3]> {
    let mut points = Vec::new();
    for level in z {
        for pj in 0..=nj {
            for pi in 0..=ni {
                points.push([pi as f64 * dx, pj as f64 * dy, *level]);
            }
        }
    }
    points
}

fn store_with(spec: &Spec) -> Store {
    let mut store = Store::default();
    store.objects.insert(CRS_ID.to_string(), Held::Crs(depth_crs()));
    store
        .objects
        .insert(TIME_CRS.to_string(), Held::TimeCrs(time_crs()));
    put_grid(&mut store, "Sugar cubes", spec);
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
fn read_with(store: &Store, trace: &[[f64; 2]], time_factor: Option<f64>) -> Result<SurfaceRead, Refused> {
    let scene = Scene {
        system: Some(&lambert()),
        time_factor,
        reprojector: &NoReprojection,
    };
    let uri = format!("eml:///resqml20.obj_IjkGridRepresentation({GRID})");
    read_surface(store, "s1", &uri, &[long(trace)], &scene)
}

/// An envelope's parts: top, bottom, fills, faults, and the cut they came in.
type EnvelopeParts<'a> = (
    &'a Spans,
    &'a Spans,
    &'a Vec<Vec<[f64; 2]>>,
    &'a Vec<Vec<[f64; 2]>>,
    &'a SurfaceCut,
);

fn envelope(read: &SurfaceRead) -> EnvelopeParts<'_> {
    assert_eq!(read.cuts.len(), 1, "the grid is met by the long section");
    let cut = &read.cuts[0];
    match &cut.shape {
        CutShape::Envelope {
            top,
            bottom,
            fills,
            faults,
        } => (top, bottom, fills, faults, cut),
        other => panic!("a grid is cut as an envelope, not {other:?}"),
    }
}

fn area(polygon: &[[f64; 2]]) -> f64 {
    let mut sum = 0.0;
    for (i, a) in polygon.iter().enumerate() {
        let b = polygon[(i + 1) % polygon.len()];
        sum += a[0] * b[1] - b[0] * a[1];
    }
    sum.abs() / 2.0
}

fn all_z(spans: &Spans, z: f64) -> bool {
    spans.iter().flatten().all(|p| (p[1] - z).abs() < 1e-9)
}

fn span_ends(span: &[[f64; 2]]) -> ([f64; 2], [f64; 2]) {
    (*span.first().unwrap(), *span.last().unwrap())
}

// -- the tests -------------------------------------------------------------

/// A grid of 4 × 1 × 2 cells, 100 m wide, layers 50 m thick (levels at 1000,
/// 950, 900), cut along `y = 50` from `x = 0` to `x = 400`: the roof is one span
/// at 1000 m from `s = 0` to `s = 400`, the floor one at 900 m, and the rock
/// between them one polygon of 400 × 100 m — **two layers, one run**.
///
/// To make it fail: pick the roof as the ring with the *lower* mean height in
/// `cut_cell` (the roof then reads 900), or cut the run at every cell (four
/// spans instead of one, four polygons instead of one).
#[test]
fn a_plain_grid_has_a_roof_a_floor_and_the_rock_between() {
    let store = store_with(&Spec {
        size: [4, 1, 2],
        points: lattice(4, 1, 100.0, 100.0, &[1000.0, 950.0, 900.0]),
        crs: CRS_ID,
        ..Default::default()
    });
    let read = read_with(&store, &[[0.0, 50.0], [400.0, 50.0]], None).unwrap();
    assert_eq!(read.object.title, "Sugar cubes");
    assert!(!read.time);

    let (top, bottom, fills, faults, cut) = envelope(&read);
    assert_eq!(top.len(), 1, "no fault, no hole: one roof {top:?}");
    assert_eq!(bottom.len(), 1);
    assert!(all_z(top, 1000.0) && all_z(bottom, 900.0), "{top:?} {bottom:?}");
    let (first, last) = span_ends(&top[0]);
    assert!(
        first[0].abs() < 1e-9 && (last[0] - 400.0).abs() < 1e-9,
        "from 0 to 400: {:?}",
        top[0]
    );
    assert!(faults.is_empty(), "a plain grid has no fault: {faults:?}");
    assert_eq!(fills.len(), 1, "two layers with no gap are one run: {fills:?}");
    assert!(
        (area(&fills[0]) - 400.0 * 100.0).abs() < 1e-6,
        "400 m by 100 m: {}",
        area(&fills[0])
    );
    assert!(cut.notes.is_empty(), "{:?}", cut.notes);
}

/// *Two faulted sugar cubes*: two cells side by side, the second down by 50 m,
/// the pillars they share **split** (`ColumnLayerSplitCoordinateLines`: pillars
/// 1 and 4 each duplicated for column 1). The section across both shows the
/// throw: the roof is two spans, 1000 m then 950 m, the floor 900 m then 850 m,
/// each pair joined by a vertical step of 50 m at `s = 100` in `faults`.
///
/// To make it fail: ignore the split lines in the reading (the throw vanishes:
/// one roof span), or let `join` extend a span across a jump in `z`.
#[test]
fn a_fault_cuts_the_roof_and_the_floor_and_its_step_is_a_fault_segment() {
    // Level 0 and level 1; lines 0..6 are pillars, 6 and 7 are the splits of
    // pillars 1 and 4. Column 0 (pillars 0, 1, 3, 4) is at 1000/900; column 1 is
    // at 950/850 through pillars 2, 5 and its split lines.
    let mut points = Vec::new();
    for (z0, z1) in [(1000.0, 950.0), (900.0, 850.0)] {
        points.extend([
            [0.0, 0.0, z0],
            [100.0, 0.0, z0],
            [200.0, 0.0, z1],
            [0.0, 100.0, z0],
            [100.0, 100.0, z0],
            [200.0, 100.0, z1],
            [100.0, 0.0, z1],
            [100.0, 100.0, z1],
        ]);
    }
    let store = store_with(&Spec {
        size: [2, 1, 1],
        points,
        splits: vec![(1, vec![1]), (4, vec![1])],
        crs: CRS_ID,
        ..Default::default()
    });
    let read = read_with(&store, &[[0.0, 50.0], [200.0, 50.0]], None).unwrap();
    let (top, bottom, fills, faults, _) = envelope(&read);

    assert_eq!(top.len(), 2, "the roof jumps at the fault: {top:?}");
    assert!(
        all_z(&vec![top[0].clone()], 1000.0) && all_z(&vec![top[1].clone()], 950.0),
        "{top:?}"
    );
    assert!((span_ends(&top[0]).1[0] - 100.0).abs() < 1e-9 && (span_ends(&top[1]).0[0] - 100.0).abs() < 1e-9);
    assert_eq!(bottom.len(), 2, "so does the floor: {bottom:?}");
    assert!(
        all_z(&vec![bottom[0].clone()], 900.0) && all_z(&vec![bottom[1].clone()], 850.0),
        "{bottom:?}"
    );

    assert_eq!(
        faults.len(),
        2,
        "one step on the roof, one on the floor: {faults:?}"
    );
    for step in faults {
        assert!(
            (step[0][0] - 100.0).abs() < 1e-9 && (step[1][0] - 100.0).abs() < 1e-9,
            "at the fault: {step:?}"
        );
        assert!(
            ((step[0][1] - step[1][1]).abs() - 50.0).abs() < 1e-9,
            "the 50 m throw: {step:?}"
        );
    }
    assert_eq!(fills.len(), 2, "the two sides are not one block: {fills:?}");
    assert!(
        fills
            .iter()
            .all(|polygon| (area(polygon) - 100.0 * 100.0).abs() < 1e-6)
    );
}

/// A column with **no defined cell** is a hole, not a fault: with the middle
/// column of three marked undefined (`CellGeometryIsDefined`, TRG §5.1, which
/// overrides whatever the pillars hold), the roof and the floor are two spans,
/// `[0, 100]` and `[200, 300]`, the rock is two polygons, and **no fault** step
/// is drawn across what is missing — though every pillar is finite.
///
/// To make it fail: let `join` extend a span across a gap in `s` (one roof span
/// bridging the hole), or read the grid without its flags (three columns).
#[test]
fn an_undefined_column_interrupts_the_roof_the_floor_and_the_fill() {
    let store = store_with(&Spec {
        size: [3, 1, 1],
        points: lattice(3, 1, 100.0, 100.0, &[1000.0, 900.0]),
        defined: Some(vec![true, false, true]),
        crs: CRS_ID,
        ..Default::default()
    });
    let read = read_with(&store, &[[0.0, 50.0], [300.0, 50.0]], None).unwrap();
    let (top, bottom, fills, faults, _) = envelope(&read);

    assert_eq!(top.len(), 2, "{top:?}");
    assert_eq!(bottom.len(), 2, "{bottom:?}");
    let spans: Vec<(f64, f64)> = top
        .iter()
        .map(|s| (span_ends(s).0[0], span_ends(s).1[0]))
        .collect();
    assert!(
        (spans[0].0).abs() < 1e-9 && (spans[0].1 - 100.0).abs() < 1e-9,
        "{spans:?}"
    );
    assert!(
        (spans[1].0 - 200.0).abs() < 1e-9 && (spans[1].1 - 300.0).abs() < 1e-9,
        "{spans:?}"
    );
    assert!(faults.is_empty(), "a hole is not a fault: {faults:?}");
    assert_eq!(fills.len(), 2, "{fills:?}");
    assert!(
        fills
            .iter()
            .all(|polygon| (area(polygon) - 100.0 * 100.0).abs() < 1e-6)
    );
}

/// A **K gap** (`KGaps`, *NKL = NK + gapCount + 1*) leaves the rock of the two
/// layers in two blocks: the roof is the top of layer 0 (1000 m), the floor the
/// bottom of layer 1 (850 m) — a K gap is not a hole in the envelope — and the
/// fill is **two** polygons of 50 m, with the 50 m void between them empty.
///
/// To make it fail: have `touches` answer `true` always (one run, one polygon of
/// 150 m, over the void), or read the grid with `Nk + 1` levels (the reader then
/// refuses the grid, and `unwrap` fails).
#[test]
fn a_k_gap_leaves_the_fill_in_two_blocks_and_the_envelope_whole() {
    let store = store_with(&Spec {
        size: [1, 1, 2],
        // Four levels: layer 0 is 1000..950, the void 950..900, layer 1 900..850.
        points: lattice(1, 1, 100.0, 100.0, &[1000.0, 950.0, 900.0, 850.0]),
        gaps: Some(vec![true]),
        crs: CRS_ID,
        ..Default::default()
    });
    let read = read_with(&store, &[[0.0, 50.0], [100.0, 50.0]], None).unwrap();
    let (top, bottom, fills, faults, _) = envelope(&read);

    assert!(all_z(top, 1000.0) && top.len() == 1, "{top:?}");
    assert!(all_z(bottom, 850.0) && bottom.len() == 1, "{bottom:?}");
    assert!(faults.is_empty(), "{faults:?}");
    assert_eq!(fills.len(), 2, "two blocks, not one over the void: {fills:?}");
    for polygon in fills {
        assert!((area(polygon) - 100.0 * 50.0).abs() < 1e-6, "{polygon:?}");
    }
}

/// A trace that passes beside the grid does not meet it: no entry (the caller's
/// `Missed`), never an empty envelope. A grid in time follows the surfaces' rule:
/// refused by name without a time factor, and carried onto the depth axis by it
/// otherwise — the CRS says z increases downward, so 1000 ms is an elevation of
/// −1000, scaled by 0.5 m per ms.
///
/// To make it fail: build the entry for every section whether or not a cell was
/// cut (the first assertion), or skip `to_scene` for a grid (the third, and the
/// refusal).
#[test]
fn a_missed_section_has_no_entry_and_a_grid_in_time_is_carried_by_the_factor() {
    let store = store_with(&Spec {
        size: [1, 1, 1],
        points: lattice(1, 1, 100.0, 100.0, &[1000.0, 1100.0]),
        crs: TIME_CRS,
        ..Default::default()
    });
    let beside = read_with(&store, &[[0.0, 500.0], [100.0, 500.0]], Some(0.5)).unwrap();
    assert!(beside.cuts.is_empty(), "{:?}", beside.cuts);

    let refused = read_with(&store, &[[0.0, 50.0], [100.0, 50.0]], None).unwrap_err();
    assert!(refused.reason.contains("time factor"), "{}", refused.reason);
    assert_eq!(refused.object.title, "Sugar cubes");

    let read = read_with(&store, &[[0.0, 50.0], [100.0, 50.0]], Some(0.5)).unwrap();
    assert!(read.time);
    let (top, bottom, _, _, _) = envelope(&read);
    assert!(
        all_z(top, -500.0),
        "1000 ms is an elevation of -1000, at 0.5: {top:?}"
    );
    assert!(all_z(bottom, -550.0), "{bottom:?}");
}

/// What the library cannot read as a grid is refused **by name**, not drawn
/// empty: a grid with no size says so, naming its title and its type.
///
/// To make it fail: return `Ok` with no cuts when the reading fails.
#[test]
fn a_grid_that_cannot_be_read_is_refused_by_name() {
    let mut store = store_with(&Spec {
        size: [1, 1, 1],
        crs: CRS_ID,
        ..Default::default()
    });
    store.objects.insert(
        GRID.to_string(),
        Held::Grid(Box::new(resqml20::IjkGridRepresentation {
            uuid: GRID.to_string(),
            citation: eml20::Citation {
                title: "Reservoir grid".into(),
                ..Default::default()
            },
            ..Default::default()
        })),
    );
    let refused = read_with(&store, &[[0.0, 50.0], [100.0, 50.0]], None).unwrap_err();
    assert!(refused.reason.contains("Reservoir grid"), "{}", refused.reason);
    assert!(
        refused.reason.contains("IjkGridRepresentation"),
        "{}",
        refused.reason
    );
    assert_eq!(
        refused.object.qualified_type,
        "resqml20.obj_IjkGridRepresentation"
    );
}
