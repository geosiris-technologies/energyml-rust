//! Wells anchored by a reference point rather than a geometry.
//!
//! Built by hand rather than read from `testdata/`, because the only real
//! package with this shape is client data that cannot be committed. The
//! objects here reproduce it exactly: a RESQML 2.2
//! `WellboreTrajectoryRepresentation` whose whole spatial content is an
//! `MdInterval` and a `ReferencePointInACrs` — including the real file's
//! contradiction, a vertical CRS naming **EPSG:5714** (a height, positive up)
//! while stating `Direction: down`.

use energyml_core::registry::DynObject;
use energyml_domain::source::ObjectSource;
use energyml_eml_2_3::{
    AbstractHorizontalCoordinates, AbstractVerticalCrs, DataObjectReference, MdInterval,
    ProjectedCoordinates, ReferencePointInACrs, VerticalCrs, VerticalDirection, VerticalEpsgCrs,
};
use energyml_geometry::{Topology, geometry, well};
use energyml_resqml_2_2::{WellboreFrameRepresentation, WellboreTrajectoryRepresentation};

const DATUM: &str = "11111111-1111-1111-1111-111111111111";
const VCRS: &str = "22222222-2222-2222-2222-222222222222";
const TRAJECTORY: &str = "33333333-3333-3333-3333-333333333333";

fn dor(uuid: &str) -> DataObjectReference {
    DataObjectReference {
        uuid: uuid.to_string(),
        ..Default::default()
    }
}

/// The BRGM shape: wellhead at (1000, 2000), ground at +37.73 m MSL,
/// drilled from MD 2.5 to MD 65.
fn datum() -> ReferencePointInACrs {
    ReferencePointInACrs {
        uuid: DATUM.to_string(),
        vertical_crs: Some(dor(VCRS)),
        vertical_coordinate: Some(37.73),
        horizontal_coordinates: Some(AbstractHorizontalCoordinates::ProjectedCoordinates(
            ProjectedCoordinates {
                coordinate1: 1000.0,
                coordinate2: 2000.0,
                crs: dor("44444444-4444-4444-4444-444444444444"),
            },
        )),
        ..Default::default()
    }
}

/// The real file's contradiction, verbatim: EPSG 5714 with `Direction: down`.
fn vertical_crs() -> VerticalCrs {
    VerticalCrs {
        uuid: VCRS.to_string(),
        direction: VerticalDirection::Down,
        abstract_vertical_crs: Some(AbstractVerticalCrs::VerticalEpsgCrs(VerticalEpsgCrs {
            epsg_code: 5714,
        })),
        ..Default::default()
    }
}

fn trajectory() -> WellboreTrajectoryRepresentation {
    WellboreTrajectoryRepresentation {
        uuid: TRAJECTORY.to_string(),
        md_interval: MdInterval {
            md_min: 2.5,
            md_max: 65.0,
            // Explicit, because `LengthUomExt::default()` is `0.1 ft` — the
            // first variant of the enum, not a sensible unit — and the reader
            // converts whatever the interval declares.
            uom: energyml_eml_2_3::LengthUomExt::M,
            datum: Some(dor(DATUM)),
            ..Default::default()
        },
        ..Default::default()
    }
}

/// A store of exactly the objects a well needs.
struct Wellsite;

impl ObjectSource for Wellsite {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        match uuid {
            DATUM => Some(Box::new(datum())),
            VCRS => Some(Box::new(vertical_crs())),
            TRAJECTORY => Some(Box::new(trajectory())),
            _ => None,
        }
    }
}

impl energyml_domain::source::ArraySource for Wellsite {
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

#[test]
fn a_trajectory_with_only_a_datum_is_a_vertical_well() {
    let found = geometry::of(&trajectory(), &Wellsite).unwrap();
    assert_eq!(found.patches.len(), 1);
    let patch = &found.patches[0];
    assert!(matches!(&patch.topology, Topology::Polylines(runs) if runs == &vec![2]));

    // Head at elevation − MdMin, bottom at elevation − MdMax — and the
    // elevation read as a **height**, because EPSG:5714 says so even though
    // the object's own `Direction` says `down`. Believing the direction would
    // put this Paris wellhead 37 m under the sea.
    assert_eq!(patch.points[0], [1000.0, 2000.0, 37.73 - 2.5]);
    assert_eq!(patch.points[1], [1000.0, 2000.0, 37.73 - 65.0]);
}

#[test]
fn the_datum_gives_the_well_its_crs() {
    // No `LocalCrs` anywhere, yet the well is fully georeferenced through the
    // reference point — the CRS layer must see through it, or the well can
    // never be exported to GeoJSON.
    let crs = energyml_domain::crs::of(&trajectory(), &Wellsite).unwrap().unwrap();
    assert_eq!(crs.vertical, energyml_domain::CrsKind::Epsg(5714));
    assert_eq!(crs.vertical_direction, energyml_domain::VerticalDirection::Up);
    // The horizontal CRS object is not in the store: stated as unknown, not
    // invented and not an error — the placement is still usable locally.
    assert_eq!(crs.projected, energyml_domain::CrsKind::Unknown);
}

#[test]
fn an_interval_upside_down_is_refused_by_name() {
    let mut wrong = trajectory();
    wrong.md_interval.md_min = 65.0;
    wrong.md_interval.md_max = 2.5;
    let error = geometry::of(&wrong, &Wellsite).unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("MdMin"), "{message}");
}

#[test]
fn a_deferred_shape_is_not_guessed_vertical() {
    // A `WitsmlTrajectory` reference means the real shape lives elsewhere;
    // drawing this one vertical would be an invention.
    let mut deferred = trajectory();
    deferred.witsml_trajectory = Some(dor("55555555-5555-5555-5555-555555555555"));
    let error = geometry::of(&deferred, &Wellsite).unwrap_err();
    assert!(format!("{error}").contains("deferred"), "{error}");
}

#[test]
fn a_frame_places_its_depths_on_the_well() {
    use energyml_eml_2_3::{AbstractFloatingPointArray, FloatingPointXmlArray};

    let frame = WellboreFrameRepresentation {
        uuid: "66666666-6666-6666-6666-666666666666".to_string(),
        node_md: Some(AbstractFloatingPointArray::FloatingPointXmlArray(
            FloatingPointXmlArray {
                count_per_value: 1,
                values: vec![10.0, 20.0, 30.0],
                ..Default::default()
            },
        )),
        trajectory: dor(TRAJECTORY),
        ..Default::default()
    };
    let found = geometry::of(&frame, &Wellsite).unwrap();
    let patch = &found.patches[0];
    assert_eq!(patch.points.len(), 3);
    assert_eq!(patch.points[1], [1000.0, 2000.0, 37.73 - 20.0]);
    assert!(matches!(&patch.topology, Topology::Polylines(runs) if runs == &vec![3]));
}

/// A frame's `intervals` attach — moved onto its nodes, never refused.
///
/// Reported from the front end: a `BSS000NSYT — formation` property had 3
/// values for 1 cell x 1 component and was not attached. `intervals`
/// is **the** element a wellbore frame's per-segment properties hang on (RESQML
/// 2.2 Usage Guide §4.2.1, Table 1, p. 38: *"intervals — wellbore frames,
/// column-layer grids"*) — `n` node depths, `n - 1` intervals — and the
/// geometry counts the frame as one polyline, one cell. Only `intervals from
/// datum` had the correspondence; this is the ordinary case.
#[test]
fn a_frames_intervals_attach_onto_its_nodes() {
    use energyml_eml_2_3::{AbstractFloatingPointArray, FloatingPointXmlArray, IndexableElement, IntegerXmlArray};
    use energyml_geometry::scene::{self, AttributeScope};
    use energyml_resqml_2_2::{AbstractValueArray, DiscreteProperty};

    const FRAME: &str = "66666666-6666-6666-6666-666666666666";
    let frame = WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
        node_md: Some(AbstractFloatingPointArray::FloatingPointXmlArray(FloatingPointXmlArray {
            count_per_value: 1,
            values: vec![0.0, 8.6, 28.6, 52.3],
            ..Default::default()
        })),
        trajectory: dor(TRAJECTORY),
        ..Default::default()
    };
    // Three passes — three formations, as codes of a lookup.
    let formation = |indexable: IndexableElement| DiscreteProperty {
        uuid: "77777777-7777-7777-7777-777777777777".to_string(),
        indexable_element: indexable,
        supporting_representation: dor(FRAME),
        values_for_patch: vec![AbstractValueArray::IntegerXmlArray(IntegerXmlArray {
            count_per_value: 1,
            values: vec![4, 7, 9],
            ..Default::default()
        })],
        ..Default::default()
    };

    let object = scene::object_of(&frame, &Wellsite, "eml:///resqml22.WellboreFrameRepresentation(66666666-6666-6666-6666-666666666666)").unwrap();
    let property = formation(IndexableElement::Intervals);
    let (found, notes) = scene::attributes_for(&object, &[&property], &Wellsite);
    assert_eq!(found.len(), 1, "attached, not refused: {notes:?}");
    assert_eq!(found[0].scope, AttributeScope::Points);
    // Each node takes the interval it opens, the deepest the one it closes —
    // and every value came from the file: no mean anywhere.
    assert_eq!(found[0].values, vec![4.0, 7.0, 9.0, 9.0]);
    assert!(notes.iter().any(|note| note.contains("moved onto")), "the correspondence is said: {notes:?}");

    // A count that fits neither the intervals nor the nodes is still refused,
    // by its numbers.
    let mut wrong = formation(IndexableElement::Intervals);
    if let AbstractValueArray::IntegerXmlArray(values) = &mut wrong.values_for_patch[0] {
        values.values = vec![4, 7];
    }
    let (found, notes) = scene::attributes_for(&object, &[&wrong], &Wellsite);
    assert!(found.is_empty(), "two values on three intervals must not attach");
    assert!(notes.iter().any(|note| note.contains("2 value(s)")), "{notes:?}");
}

// ---------------------------------------------------------------------------
// A frame's head portion — MD 0 to its first node, never stated by any
// object (seen on a SIGEOL import).

fn frame_from(mds: Vec<f64>) -> WellboreFrameRepresentation {
    use energyml_eml_2_3::{AbstractFloatingPointArray, FloatingPointXmlArray};
    WellboreFrameRepresentation {
        uuid: "66666666-6666-6666-6666-666666666666".to_string(),
        node_md: Some(AbstractFloatingPointArray::FloatingPointXmlArray(FloatingPointXmlArray {
            count_per_value: 1,
            values: mds,
            ..Default::default()
        })),
        trajectory: dor(TRAJECTORY),
        ..Default::default()
    }
}

/// The vertical case, matching `BSS000NNHD`: a trajectory with only a datum
/// (no `Geometry`), a frame whose first node is below the wellhead. The head
/// portion is the straight segment `Anchor::at` already draws for the rest of
/// this file — same formula, so the seam at the first node cannot show.
#[test]
fn a_frames_head_portion_runs_from_the_datum_to_its_first_node() {
    let frame = frame_from(vec![10.0, 20.0, 30.0]);
    let found = well::head_portion(&frame, &Wellsite).unwrap();
    let points = found.expect("MD 10 is below the datum: there is a portion to draw");
    assert_eq!(points, vec![[1000.0, 2000.0, 37.73], [1000.0, 2000.0, 37.73 - 10.0]]);
}

/// A frame that already starts at MD 0 has nothing missing: `None`, not a
/// segment of zero length that would draw as a stray dot.
#[test]
fn a_frame_starting_at_md_0_has_no_head_portion() {
    let frame = frame_from(vec![0.0, 8.6, 28.6]);
    let found = well::head_portion(&frame, &Wellsite).unwrap();
    assert!(found.is_none(), "nothing is missing above MD 0");
}

/// Neither the trajectory's shape nor its datum can be read: a named refusal,
/// never a guess at where the well's head might be.
#[test]
fn an_unreadable_datum_is_refused_by_name() {
    struct BrokenSite(WellboreTrajectoryRepresentation);
    impl ObjectSource for BrokenSite {
        fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
            (uuid == TRAJECTORY).then(|| Box::new(self.0.clone()) as Box<dyn DynObject>)
        }
    }
    impl energyml_domain::source::ArraySource for BrokenSite {
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

    let mut broken = trajectory();
    broken.md_interval.datum = None;
    let site = BrokenSite(broken);
    let frame = frame_from(vec![10.0, 20.0]);
    let error = well::head_portion(&frame, &site).unwrap_err();
    let message = format!("{error}");
    assert!(message.contains("head portion"), "{message}");
}

/// A deviated well: the head portion **follows the trajectory's own shape**
/// rather than a straight guess, sampled with the exact reader `frame` uses
/// for its own nodes (`Line::at`), so the two never disagree at the seam.
///
/// `LineKindIndex` 1 — linear spline — over two control points at
/// `(1000, 2000, 37.73)` (MD 0) and `(1010, 2000, -62.27)` (MD 100), so a
/// point at any MD is an exact linear interpolation the test can compute by
/// hand.
#[test]
fn a_deviated_frames_head_portion_follows_the_trajectorys_shape() {
    use energyml_eml_2_3::{AbstractFloatingPointArray, FloatingPointXmlArray};
    use energyml_resqml_2_2::{
        AbstractParametricLineGeometry, AbstractPoint3dArray, ParametricLineGeometry, Point3d,
        Point3dLatticeArray, Point3dLatticeDimension,
    };

    const DEVIATED: &str = "88888888-8888-8888-8888-888888888888";
    let origin = Point3d { coordinate1: 1000.0, coordinate2: 2000.0, coordinate3: 37.73 };
    let direction = Point3d { coordinate1: 10.0, coordinate2: 0.0, coordinate3: -100.0 };

    let deviated = WellboreTrajectoryRepresentation {
        uuid: DEVIATED.to_string(),
        md_interval: MdInterval {
            md_min: 0.0,
            md_max: 100.0,
            uom: energyml_eml_2_3::LengthUomExt::M,
            datum: Some(dor(DATUM)),
            ..Default::default()
        },
        geometry: Some(AbstractParametricLineGeometry::ParametricLineGeometry(ParametricLineGeometry {
            local_crs: dor(VCRS),
            control_point_parameters: Some(AbstractFloatingPointArray::FloatingPointXmlArray(
                FloatingPointXmlArray { count_per_value: 1, values: vec![0.0, 100.0], ..Default::default() },
            )),
            control_points: Some(AbstractPoint3dArray::Point3dLatticeArray(Point3dLatticeArray {
                all_dimensions_are_orthogonal: None,
                origin,
                dimension: vec![Point3dLatticeDimension {
                    direction,
                    spacing: Some(AbstractFloatingPointArray::FloatingPointXmlArray(FloatingPointXmlArray {
                        count_per_value: 1,
                        values: vec![1.0],
                        ..Default::default()
                    })),
                }],
            })),
            knot_count: 2,
            line_kind_index: 1,
            ..Default::default()
        })),
        ..Default::default()
    };

    struct DeviatedSite(WellboreTrajectoryRepresentation);
    impl ObjectSource for DeviatedSite {
        fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
            match uuid {
                DATUM => Some(Box::new(datum())),
                VCRS => Some(Box::new(vertical_crs())),
                DEVIATED => Some(Box::new(self.0.clone())),
                _ => None,
            }
        }
    }
    impl energyml_domain::source::ArraySource for DeviatedSite {
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

    let frame = WellboreFrameRepresentation {
        uuid: "99999999-9999-9999-9999-999999999999".to_string(),
        node_md: Some(AbstractFloatingPointArray::FloatingPointXmlArray(FloatingPointXmlArray {
            count_per_value: 1,
            values: vec![40.0, 60.0],
            ..Default::default()
        })),
        trajectory: dor(DEVIATED),
        ..Default::default()
    };

    let site = DeviatedSite(deviated);
    let found = well::head_portion(&frame, &site).unwrap();
    let points = found.expect("MD 40 is well below MD 0: there is a portion to draw");
    assert_eq!(points.len(), 17, "one sample per HEAD_SAMPLES step");
    assert_eq!(points[0], [1000.0, 2000.0, 37.73], "MD 0 is the origin control point exactly");
    // MD 20 (i = 8 of 16): t = 20 / 100 = 0.2 along the line.
    assert!((points[8][0] - 1002.0).abs() < 1e-9);
    assert!((points[8][2] - 17.73).abs() < 1e-9);
    // MD 40 (i = 16, the last sample): t = 0.4.
    assert!((points[16][0] - 1004.0).abs() < 1e-9);
    assert!((points[16][2] - (-2.27)).abs() < 1e-9);
}
