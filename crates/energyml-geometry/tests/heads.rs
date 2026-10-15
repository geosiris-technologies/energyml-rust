//! `heads::of_objects` on objects built by hand after the specification: a
//! wellbore trajectory is collared where the datum of its `MdInterval` is
//! (RESQML TRG 2.2 §12.17, *"the MdDatum within the MdInterval is mandatory"*),
//! and a frame hangs on its trajectory (§12.13).

use energyml_core::registry::DynObject;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_eml_2_3::{
    AbstractHorizontalCoordinates, AbstractProjectedCrs, AbstractVerticalCrs, DataObjectReference,
    MdInterval, ProjectedCoordinates, ProjectedCrs, ProjectedEpsgCrs, ReferencePointInACrs, VerticalCrs,
    VerticalDirection, VerticalEpsgCrs,
};
use energyml_geometry::{Placement, heads};
use energyml_resqml_2_2::{WellboreFrameRepresentation, WellboreTrajectoryRepresentation};

const VCRS: &str = "22222222-2222-2222-2222-222222222222";
const PCRS_A: &str = "aaaaaaaa-0000-0000-0000-00000000000a";
const PCRS_B: &str = "bbbbbbbb-0000-0000-0000-00000000000b";
const DATUM_A: &str = "11111111-1111-1111-1111-11111111111a";
const DATUM_B: &str = "11111111-1111-1111-1111-11111111111b";
const TRAJ_A: &str = "33333333-3333-3333-3333-33333333333a";
const TRAJ_B: &str = "33333333-3333-3333-3333-33333333333b";
const FRAME_A: &str = "66666666-6666-6666-6666-66666666666a";
const LOOSE_FRAME: &str = "66666666-6666-6666-6666-66666666666c";

fn dor(uuid: &str) -> DataObjectReference {
    DataObjectReference {
        uuid: uuid.to_string(),
        ..Default::default()
    }
}

fn datum(uuid: &str, crs: &str, x: f64, y: f64) -> ReferencePointInACrs {
    ReferencePointInACrs {
        uuid: uuid.to_string(),
        vertical_crs: Some(dor(VCRS)),
        vertical_coordinate: Some(37.73),
        horizontal_coordinates: Some(AbstractHorizontalCoordinates::ProjectedCoordinates(
            ProjectedCoordinates {
                coordinate1: x,
                coordinate2: y,
                crs: dor(crs),
            },
        )),
        ..Default::default()
    }
}

fn projected(uuid: &str, epsg: i64) -> ProjectedCrs {
    ProjectedCrs {
        uuid: uuid.to_string(),
        abstract_projected_crs: Some(AbstractProjectedCrs::ProjectedEpsgCrs(ProjectedEpsgCrs {
            epsg_code: epsg,
        })),
        ..Default::default()
    }
}

fn trajectory(uuid: &str, datum: &str) -> WellboreTrajectoryRepresentation {
    WellboreTrajectoryRepresentation {
        uuid: uuid.to_string(),
        md_interval: MdInterval {
            md_min: 0.0,
            md_max: 65.0,
            uom: energyml_eml_2_3::LengthUomExt::M,
            datum: Some(dor(datum)),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn frame(uuid: &str, trajectory: &str) -> WellboreFrameRepresentation {
    WellboreFrameRepresentation {
        uuid: uuid.to_string(),
        trajectory: dor(trajectory),
        ..Default::default()
    }
}

/// Two wells in two systems (EPSG 2154 and 27572), a frame on the first, a
/// frame whose trajectory is absent, and a vertical CRS - an object that is no
/// well at all.
struct Package;

impl ObjectSource for Package {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        Some(match uuid {
            VCRS => Box::new(VerticalCrs {
                uuid: VCRS.to_string(),
                direction: VerticalDirection::Up,
                abstract_vertical_crs: Some(AbstractVerticalCrs::VerticalEpsgCrs(VerticalEpsgCrs {
                    epsg_code: 5714,
                })),
                ..Default::default()
            }),
            PCRS_A => Box::new(projected(PCRS_A, 2154)),
            PCRS_B => Box::new(projected(PCRS_B, 27572)),
            DATUM_A => Box::new(datum(DATUM_A, PCRS_A, 1000.0, 2000.0)),
            DATUM_B => Box::new(datum(DATUM_B, PCRS_B, 5000.0, 6000.0)),
            TRAJ_A => Box::new(trajectory(TRAJ_A, DATUM_A)),
            TRAJ_B => Box::new(trajectory(TRAJ_B, DATUM_B)),
            FRAME_A => Box::new(frame(FRAME_A, TRAJ_A)),
            LOOSE_FRAME => Box::new(frame(LOOSE_FRAME, "99999999-9999-9999-9999-999999999999")),
            _ => return None,
        })
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

fn ask(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|one| one.to_string()).collect()
}

/// A trajectory is collared at its datum, and a frame at **its trajectory's**
/// datum: both read the same point, and the answer names the object as asked.
///
/// Casser : in `heads.rs`, read a frame's head from the frame itself (`root`
/// instead of `followed`) - the frame is refused as having no `MdInterval`; or
/// swap `x` and `y` in `place`.
#[test]
fn a_trajectory_and_its_frame_are_collared_at_the_same_datum() {
    let found = heads::of_objects(&Package, &ask(&[TRAJ_A, FRAME_A]), Placement::Projected).unwrap();
    assert!(found.refused.is_empty(), "{:?}", found.refused);
    assert_eq!(found.heads.len(), 2);
    assert_eq!(found.heads[0].id, TRAJ_A);
    assert_eq!(found.heads[1].id, FRAME_A);
    assert_eq!(found.heads[0].xy, [1000.0, 2000.0]);
    assert_eq!(found.heads[1].xy, [1000.0, 2000.0]);
    assert_eq!(found.system, Some(2154), "the system the CRS object states");
}

/// An object that is no well is refused **by its type** and the lot goes on;
/// so is a frame whose trajectory the package does not hold, and an id the
/// package does not know. Nothing is placed at the origin instead.
///
/// Casser : in `heads.rs`, drop the `other =>` arm's `return Err` (let any
/// object through to `head_of`) - the type is no longer named; or turn a
/// refusal into `[0, 0]` - the three refusals vanish.
#[test]
fn what_is_no_well_is_refused_by_name_and_the_others_go_on() {
    let found = heads::of_objects(
        &Package,
        &ask(&[VCRS, TRAJ_A, LOOSE_FRAME, "nope"]),
        Placement::Projected,
    )
    .unwrap();
    assert_eq!(found.heads.len(), 1);
    assert_eq!(found.heads[0].id, TRAJ_A);
    assert_eq!(found.refused.len(), 3);
    assert_eq!(found.refused[0].id, VCRS);
    assert!(
        found.refused[0].why.contains("VerticalCrs"),
        "{}",
        found.refused[0].why
    );
    assert_eq!(found.refused[1].id, LOOSE_FRAME);
    assert!(
        found.refused[1].why.contains("not in this package"),
        "{}",
        found.refused[1].why
    );
    assert_eq!(found.refused[2].id, "nope");
}

/// Two heads in two systems are **refused as a lot**, naming the second well
/// and both systems: measuring them in one plane would mix two frames.
///
/// Casser : in `of_objects`, drop the `*first != system` arm - the lot answers
/// with one head in 27572 beside one in 2154, and this test fails on
/// `unwrap_err`.
#[test]
fn heads_in_two_systems_are_refused() {
    let error = heads::of_objects(&Package, &ask(&[TRAJ_A, TRAJ_B]), Placement::Projected).unwrap_err();
    let message = format!("{error}");
    assert!(
        message.contains("EPSG:27572") && message.contains("EPSG:2154"),
        "{message}"
    );
    assert!(message.contains(TRAJ_A), "the first well is named: {message}");
    // Each alone is fine: the refusal is about the pairing.
    assert!(heads::of_objects(&Package, &ask(&[TRAJ_B]), Placement::Projected).is_ok());
}

/// `trajectories_of` is one uri per trajectory and never a frame - a frame
/// would repeat its trajectory's head.
///
/// Casser : match `Representation` instead of `…TrajectoryRepresentation`.
#[test]
fn every_well_means_the_trajectories() {
    use energyml_storage::resource::ObjectSummary;
    let summary = |uri: &str, qualified_type: &str| {
        ObjectSummary::new(uri.to_string(), String::new(), qualified_type.to_string())
    };
    let all = [
        summary("a", "resqml22.WellboreTrajectoryRepresentation"),
        summary("b", "resqml22.WellboreFrameRepresentation"),
        summary("c", "resqml20.obj_WellboreTrajectoryRepresentation"),
        summary("d", "resqml22.TriangulatedSetRepresentation"),
    ];
    assert_eq!(
        heads::trajectories_of(&all),
        vec!["a".to_string(), "c".to_string()]
    );
}
