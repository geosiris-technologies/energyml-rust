//! The head portion of a frame whose trajectory has a `Geometry` that starts
//! below MD 0, on RESQML 2.0.1 objects (the flat `StartMd` / `MdDatum` shape,
//! a `MdDatum` that carries its `Location` beside its `LocalCrs`).
//!
//! Built by hand, no file read. The facts the expected numbers rest on:
//!
//! - MD 0 is at the datum: *"BUSINESS RULE: For purposes of the trajectory the
//!   MdDatum within the MdInterval is mandatory"* (RESQML TRG 2.2 §12.17
//!   p. 314);
//! - the offsets of a local CRS and `ZIncreasingDownward` (RESQML TRG 2.0.1
//!   §2.1 p. 10-11);
//! - a parametric line is unbounded: past its ends, its first or last interval
//!   is extended (RESQML TRG 2.2 §3.8).
//!
//! Every unit is written: the default of a unit enumeration is its first
//! variant, never a sensible unit.

use energyml_core::registry::DynObject;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_0::{
    AbstractProjectedCrs, AbstractVerticalCrs, DataObjectReference, LengthUom, PlaneAngleMeasure,
    PlaneAngleUom, ProjectedCrsEpsgCode, ProjectedUnknownCrs, VerticalCrsEpsgCode, VerticalUnknownCrs,
};
use energyml_geometry::geometry;
use energyml_geometry::scene::{self, Placement};
use energyml_geometry::well::{self, HeadPortion};
use energyml_resqml_2_0_1::{
    AbstractDoubleArray, AbstractParametricLineGeometry, AbstractPoint3dArray, DoubleConstantArray,
    DoubleLatticeArray, LocalDepth3dCrs, MdDatum, MdReference, ParametricLineGeometry, Point3d,
    Point3dLatticeArray, Point3dOffset, WellboreFrameRepresentation, WellboreTrajectoryRepresentation,
};

const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const B: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const D: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const T: &str = "11111111-1111-4111-8111-111111111111";
const F: &str = "ffffffff-ffff-4fff-8fff-ffffffffffff";

fn dor(uuid: &str) -> DataObjectReference {
    DataObjectReference {
        uuid: uuid.to_string(),
        ..Default::default()
    }
}

/// A local depth CRS counting downward, in metres horizontally. `None` for a
/// system makes it `...UnknownCrs`.
fn crs(
    uuid: &str,
    offsets: (f64, f64, f64),
    projected: Option<u64>,
    vertical: Option<u64>,
    vertical_uom: LengthUom,
) -> LocalDepth3dCrs {
    LocalDepth3dCrs {
        uuid: uuid.to_string(),
        x_offset: offsets.0,
        y_offset: offsets.1,
        z_offset: offsets.2,
        areal_rotation: PlaneAngleMeasure {
            value: 0.0,
            uom: PlaneAngleUom::Dega,
        },
        projected_uom: LengthUom::M,
        vertical_uom,
        z_increasing_downward: true,
        projected_crs: Some(match projected {
            Some(epsg_code) => AbstractProjectedCrs::ProjectedCrsEpsgCode(ProjectedCrsEpsgCode { epsg_code }),
            None => AbstractProjectedCrs::ProjectedUnknownCrs(ProjectedUnknownCrs {
                unknown: "unknown".to_string(),
            }),
        }),
        vertical_crs: Some(match vertical {
            Some(epsg_code) => AbstractVerticalCrs::VerticalCrsEpsgCode(VerticalCrsEpsgCode { epsg_code }),
            None => AbstractVerticalCrs::VerticalUnknownCrs(VerticalUnknownCrs {
                unknown: "unknown".to_string(),
            }),
        }),
        ..Default::default()
    }
}

/// The CRS of the line: offset (1000, 2000), metres, EPSG 2154 and 5714.
fn crs_a() -> LocalDepth3dCrs {
    crs(A, (1000.0, 2000.0, 0.0), Some(2154), Some(5714), LengthUom::M)
}

/// A datum written at `at` in the CRS `in_crs`: the `Location` beside the
/// `LocalCrs`, as RESQML 2.0.1 writes it.
fn datum(in_crs: &str, at: [f64; 3]) -> MdDatum {
    MdDatum {
        uuid: D.to_string(),
        location: Point3d {
            coordinate1: at[0],
            coordinate2: at[1],
            coordinate3: at[2],
        },
        md_reference: MdReference::KellyBushing,
        local_crs: dor(in_crs),
        ..Default::default()
    }
}

/// A trajectory whose line is `L(md) = (0, 0, z0) + (md - start_md) / 50 *
/// (30, 0, 40)` (a step of 50 along a direction of length 50: the parameter is
/// the length along the line), starting at MD `start_md`, in the CRS `line_crs`.
fn trajectory(start_md: f64, z0: f64, line_crs: &str) -> WellboreTrajectoryRepresentation {
    WellboreTrajectoryRepresentation {
        uuid: T.to_string(),
        start_md,
        finish_md: start_md + 50.0,
        md_uom: LengthUom::M,
        md_datum: dor(D),
        geometry: Some(AbstractParametricLineGeometry::ParametricLineGeometry(
            ParametricLineGeometry {
                local_crs: dor(line_crs),
                control_point_parameters: Some(AbstractDoubleArray::DoubleLatticeArray(DoubleLatticeArray {
                    start_value: start_md,
                    offset: vec![DoubleConstantArray {
                        value: 50.0,
                        count: 1,
                    }],
                })),
                control_points: Some(AbstractPoint3dArray::Point3dLatticeArray(Point3dLatticeArray {
                    all_dimensions_are_orthogonal: None,
                    origin: Point3d {
                        coordinate1: 0.0,
                        coordinate2: 0.0,
                        coordinate3: z0,
                    },
                    offset: vec![Point3dOffset {
                        offset: Point3d {
                            coordinate1: 30.0,
                            coordinate2: 0.0,
                            coordinate3: 40.0,
                        },
                        spacing: Some(AbstractDoubleArray::DoubleConstantArray(DoubleConstantArray {
                            value: 1.0,
                            count: 1,
                        })),
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

/// A frame of two nodes: MD `first` and MD 60.
fn frame(first: f64) -> WellboreFrameRepresentation {
    WellboreFrameRepresentation {
        uuid: F.to_string(),
        node_count: 2,
        node_md: Some(AbstractDoubleArray::DoubleLatticeArray(DoubleLatticeArray {
            start_value: first,
            offset: vec![DoubleConstantArray {
                value: 60.0 - first,
                count: 1,
            }],
        })),
        trajectory: dor(T),
        ..Default::default()
    }
}

/// What the store holds: the CRS objects, the datum (when there is one) and the
/// trajectory.
struct Site {
    crs: Vec<LocalDepth3dCrs>,
    datum: Option<MdDatum>,
    trajectory: WellboreTrajectoryRepresentation,
}

impl ObjectSource for Site {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        if let Some(found) = self.crs.iter().find(|c| c.uuid.eq_ignore_ascii_case(uuid)) {
            return Some(Box::new(found.clone()));
        }
        if uuid.eq_ignore_ascii_case(D) {
            return self.datum.clone().map(|d| Box::new(d) as Box<dyn DynObject>);
        }
        if uuid.eq_ignore_ascii_case(T) {
            return Some(Box::new(self.trajectory.clone()));
        }
        None
    }
}

impl ArraySource for Site {
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

const FRAME_URI: &str =
    "eml:///resqml20.obj_WellboreFrameRepresentation(ffffffff-ffff-4fff-8fff-ffffffffffff)";

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
}

/// The line starts at MD 10, the datum is in the line's own CRS: the portion is
/// the datum, then the line itself, in the frame of the line, and a caller
/// places it with the same chain as the nodes.
///
/// Breaking it: (1) the old arm (`Anchor::at`, a datum already placed) rejects
/// or gives two points; (2) bringing `raw` through `A.to_projected` (the datum
/// placed twice, the defect D7) gives `placed[0] = (2000, 4000, 0)`.
#[test]
fn a_line_starting_below_md_0_is_joined_to_the_datum_in_its_own_frame() {
    let site = Site {
        crs: vec![crs_a()],
        datum: Some(datum(A, [0.0, 0.0, 0.0])),
        trajectory: trajectory(10.0, 10.0, A),
    };
    let frame = frame(35.0);
    let head = well::head_portion(&frame, &site)
        .unwrap()
        .expect("MD 35 is below the datum");
    assert!(
        matches!(head, HeadPortion::Assumed(_)),
        "the chord is stated by no object"
    );
    assert_eq!(head.points().len(), 18, "the datum, then 17 samples of the line");
    // The datum is written in the line's frame: no calculation at all.
    assert_eq!(head.points()[0], [0.0, 0.0, 0.0]);
    // Where the line starts: MD 10.
    assert!(close(head.points()[1], [0.0, 0.0, 10.0]));
    // MD 22.5 (the 8th step from MD 10 to MD 35).
    assert!(close(head.points()[9], [7.5, 0.0, 20.0]));
    // MD 35: the frame's first node, to the bit.
    assert!(close(head.points()[17], [15.0, 0.0, 30.0]));
    let nodes = geometry::of(&frame, &site).unwrap();
    assert_eq!(head.points()[17], nodes.patches[0].points[0]);

    // The caller's chain: the same `place_points` for the nodes and the portion.
    let object = scene::object_of(&frame, &site, FRAME_URI).unwrap();
    let placed = object
        .place_points(head.into_points(), Placement::Projected)
        .unwrap();
    // A: x + 1000, y + 2000, z -> -z.
    assert!(close(placed[0], [1000.0, 2000.0, 0.0]), "{:?}", placed[0]);
    assert!(close(placed[17], [1015.0, 2000.0, -30.0]), "{:?}", placed[17]);
}

/// The datum is written in another CRS of the same systems: it is brought into
/// the line's frame.
///
/// B: offsets (1010, 1990), z offset 2, counting down. The datum at
/// `(-10, 10, -2)` in B is `(1000, 2000, 0)` projected, which is `(0, 0, 0)` in
/// A. Breaking it: skip the conversion and return `raw`, giving `(-10, 10, -2)`.
#[test]
fn a_datum_in_another_local_crs_is_brought_into_the_lines_frame() {
    let site = Site {
        crs: vec![
            crs_a(),
            crs(B, (1010.0, 1990.0, 2.0), Some(2154), Some(5714), LengthUom::M),
        ],
        datum: Some(datum(B, [-10.0, 10.0, -2.0])),
        trajectory: trajectory(10.0, 10.0, A),
    };
    let frame = frame(35.0);
    let head = well::head_portion(&frame, &site).unwrap().unwrap();
    assert!(close(head.points()[0], [0.0, 0.0, 0.0]), "{:?}", head.points()[0]);
    let object = scene::object_of(&frame, &site, FRAME_URI).unwrap();
    let placed = object
        .place_points(head.into_points(), Placement::Projected)
        .unwrap();
    assert!(close(placed[0], [1000.0, 2000.0, 0.0]), "{:?}", placed[0]);
}

/// A datum whose vertical axis counts feet: its elevation is brought to the
/// line's unit (metres). `10 ft` below the datum's origin is `3.048 m` deep,
/// so `z = -3.048` in A. Breaking it: remove the conversion, `z = -10`.
///
/// `TODO(verify)`: the factor of `units::metres(1.0, "ft")` is `0.3048`.
#[test]
fn a_datum_counted_in_feet_is_brought_to_the_lines_unit() {
    let site = Site {
        crs: vec![
            crs_a(),
            crs(B, (1000.0, 2000.0, 0.0), Some(2154), Some(5714), LengthUom::Ft),
        ],
        datum: Some(datum(B, [0.0, 0.0, -10.0])),
        trajectory: trajectory(10.0, 10.0, A),
    };
    let head = well::head_portion(&frame(35.0), &site).unwrap().unwrap();
    assert!(
        close(head.points()[0], [0.0, 0.0, -3.048]),
        "{:?}",
        head.points()[0]
    );
}

/// What cannot be proved the same system is refused, by name: another
/// projected system, two unknown systems (two unknowns prove nothing).
///
/// Breaking it: `same_system` answering `true` for two `Unknown` makes (ii)
/// succeed; deleting the check makes (i) and (ii) succeed.
#[test]
fn a_datum_that_cannot_join_the_lines_frame_is_refused_by_name() {
    // (i) another projected system.
    let site = Site {
        crs: vec![
            crs_a(),
            crs(B, (1010.0, 1990.0, 2.0), Some(32631), Some(5714), LengthUom::M),
        ],
        datum: Some(datum(B, [-10.0, 10.0, -2.0])),
        trajectory: trajectory(10.0, 10.0, A),
    };
    let error = well::head_portion(&frame(35.0), &site).unwrap_err().to_string();
    for needle in ["head portion", "MdMin 10", "EPSG:32631", "EPSG:2154"] {
        assert!(error.contains(needle), "`{needle}` in: {error}");
    }

    // (ii) both vertical systems unknown, in two distinct objects.
    let site = Site {
        crs: vec![
            crs(A, (1000.0, 2000.0, 0.0), Some(2154), None, LengthUom::M),
            crs(B, (1010.0, 1990.0, 2.0), Some(2154), None, LengthUom::M),
        ],
        datum: Some(datum(B, [-10.0, 10.0, -2.0])),
        trajectory: trajectory(10.0, 10.0, A),
    };
    let error = well::head_portion(&frame(35.0), &site).unwrap_err().to_string();
    assert!(error.contains("an unknown system"), "{error}");
}

/// A line that reaches MD 0 needs no datum: the package describes the portion.
///
/// Breaking it: read the datum before testing `MdMin`, the missing datum is
/// then an error.
#[test]
fn a_line_reaching_md_0_needs_no_datum() {
    let site = Site {
        crs: vec![crs_a()],
        datum: None,
        trajectory: trajectory(0.0, 0.0, A),
    };
    let head = well::head_portion(&frame(35.0), &site).unwrap().unwrap();
    assert!(head.is_described());
    assert_eq!(head.points().len(), 17);
    // MD 17.5 = (17.5 / 50) * (30, 0, 40).
    assert!(
        close(head.points()[8], [10.5, 0.0, 14.0]),
        "{:?}",
        head.points()[8]
    );
    // MD 35.
    assert!(
        close(head.points()[16], [21.0, 0.0, 28.0]),
        "{:?}",
        head.points()[16]
    );
}

/// The first node above the start of the line: `frame` places it by extending
/// the first interval, so the portion ends on that point and does not climb
/// back from MD 10 to MD 5.
///
/// Breaking it: `e = md_min` instead of `min(md_min, first_md)` gives 18 points
/// that go up from MD 10 to MD 5.
#[test]
fn a_first_node_above_the_lines_start_joins_the_datum_straight() {
    let site = Site {
        crs: vec![crs_a()],
        datum: Some(datum(A, [0.0, 0.0, 0.0])),
        trajectory: trajectory(10.0, 10.0, A),
    };
    let frame = frame(5.0);
    let head = well::head_portion(&frame, &site).unwrap().unwrap();
    // L(5) = (0, 0, 10) - 5/50 * (30, 0, 40) = (-3, 0, 6).
    let first_node = geometry::of(&frame, &site).unwrap().patches[0].points[0];
    assert!(close(first_node, [-3.0, 0.0, 6.0]), "{first_node:?}");
    assert_eq!(head.points(), &[[0.0, 0.0, 0.0], first_node][..]);
    assert!(!head.is_described());
}
