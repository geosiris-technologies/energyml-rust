//! A borrowed coordinate system, when the lender is in a **list**.
//!
//! Reported on `volve.epc`: the `GridConnectionSetRepresentation` `2efbb020`
//! came out misplaced, "as if the CRS were not applied". It was not -
//! `energyml geometry` said `[no CRS]` on 601 560 points.
//!
//! A connection set carries no geometry: its points are those of the grids it
//! joins, and its system is theirs. The borrowing mechanism existed
//! (`BORROWED_GEOMETRY_FIELDS`) and could not see it, for two reasons that had to
//! be fixed **both**:
//!
//! - `Grid` was not in the list of lender fields;
//! - and the loop accepted only a `FieldValue::Object`, whereas a connection set
//!   names its grids in a **list** of references.
//!
//! These tests hold both, plus the rule that bounds them: **if the lenders do not
//! agree, nothing is borrowed.** Two grids in two systems are two systems, and
//! taking the first would be "a guess dressed up as a number" - the phrase
//! `crs.rs` already uses for the neighbouring case of a 2.2 sub-representation
//! drawing on several supports.

use energyml_core::registry::DynObject;
use energyml_domain::crs;
use energyml_domain::source::ObjectSource;
use energyml_eml_2_0 as eml20;
use energyml_resqml_2_0_1 as resqml20;

const GRID_A: &str = "aaaaaaaa-0000-4000-8000-000000000001";
const GRID_B: &str = "aaaaaaaa-0000-4000-8000-000000000002";
const CRS_ONE: &str = "bbbbbbbb-0000-4000-8000-000000000001";
const CRS_TWO: &str = "bbbbbbbb-0000-4000-8000-000000000002";

/// A grid that names a CRS, and the CRS itself.
struct Store;

impl Store {
    fn grid(uuid: &str, crs_uuid: &str) -> Box<dyn DynObject> {
        Box::new(resqml20::IjkGridRepresentation {
            uuid: uuid.to_string(),
            schema_version: "2.0".into(),
            geometry: Some(resqml20::IjkGridGeometry {
                local_crs: eml20::DataObjectReference {
                    uuid: crs_uuid.to_string(),
                    content_type: "application/x-resqml+xml;version=2.0;type=obj_LocalDepth3dCrs".into(),
                    ..Default::default()
                },
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    fn crs(uuid: &str, epsg: u64) -> Box<dyn DynObject> {
        Box::new(resqml20::LocalDepth3dCrs {
            uuid: uuid.to_string(),
            schema_version: "2.0".into(),
            projected_crs: Some(eml20::AbstractProjectedCrs::ProjectedCrsEpsgCode(
                eml20::ProjectedCrsEpsgCode { epsg_code: epsg },
            )),
            ..Default::default()
        })
    }
}

impl ObjectSource for Store {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        match uuid {
            GRID_A => Some(Store::grid(GRID_A, CRS_ONE)),
            GRID_B => Some(Store::grid(GRID_B, CRS_TWO)),
            CRS_ONE => Some(Store::crs(CRS_ONE, 23031)),
            CRS_TWO => Some(Store::crs(CRS_TWO, 32631)),
            _ => None,
        }
    }
}

/// A connection set naming the given grids.
fn connections(grids: &[&str]) -> resqml20::GridConnectionSetRepresentation {
    resqml20::GridConnectionSetRepresentation {
        uuid: "cccccccc-0000-4000-8000-000000000001".into(),
        schema_version: "2.0".into(),
        count: 1,
        grid: grids
            .iter()
            .map(|uuid| eml20::DataObjectReference {
                uuid: (*uuid).to_string(),
                content_type: "application/x-resqml+xml;version=2.0;type=obj_IjkGridRepresentation".into(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn a_connection_set_takes_the_system_of_its_grid() {
    // The Volve defect: 601 560 points drawn where the file does not put them.
    // The set carries no geometry, hence no `LocalCrs` - its system is the
    // grid's, and it has to be fetched.
    let read = crs::of(&connections(&[GRID_A]), &Store)
        .expect("the read succeeds")
        .expect("a system borrowed from the grid");
    assert_eq!(read.projected.epsg(), Some(23031));
}

#[test]
fn two_grids_of_the_same_system_agree() {
    let read = crs::of(&connections(&[GRID_A, GRID_A]), &Store)
        .expect("the read succeeds")
        .expect("both lenders say the same thing");
    assert_eq!(read.projected.epsg(), Some(23031));
}

#[test]
fn two_grids_in_disagreement_borrow_nothing() {
    // **The bound, and it bites.** Taking the first would be a guess dressed up
    // as a number: a set placed in one of the two systems is wrong for the
    // other, and nothing on screen would say so. Answering nothing at least
    // sends someone looking.
    let read = crs::of(&connections(&[GRID_A, GRID_B]), &Store).expect("the read succeeds");
    assert!(
        read.is_none(),
        "two different systems do not make one system: {read:?}"
    );
}

#[test]
fn a_grid_absent_from_the_package_does_not_fail_the_read() {
    // A package may legitimately reference a grid it does not hold. That is not
    // an error - it is an absence of answer.
    let read = crs::of(&connections(&["dddddddd-0000-4000-8000-000000000009"]), &Store)
        .expect("the read succeeds despite the dangling reference");
    assert!(read.is_none());
}

/// A RESQML 2.2 sub-representation: its supports are on its **patches**.
fn sub_representation(supports: &[&str]) -> energyml_resqml_2_2::SubRepresentation {
    energyml_resqml_2_2::SubRepresentation {
        uuid: "eeeeeeee-0000-4000-8000-000000000001".into(),
        schema_version: "2.2".into(),
        sub_representation_patch: supports
            .iter()
            .map(|uuid| energyml_resqml_2_2::SubRepresentationPatch {
                supporting_representation: energyml_eml_2_3::DataObjectReference {
                    uuid: (*uuid).to_string(),
                    qualified_type: "resqml22.IjkGridRepresentation".into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

#[test]
fn a_2_2_sub_representation_takes_the_system_of_its_patches_support() {
    // Synthetic reference model: "Productive reservoir - Chalk" (a
    // `SubRepresentation` of the Ypresian-Chalk grid) had no system, and the
    // scene refused it. RESQML TRG 2.2 §8.10 p. 247: the support is
    // `SubRepresentationPatch.SupportingRepresentation`, not a field of the object.
    // To break it: remove the call to `patch_supports_agree` in `crs::of_within`.
    let read = crs::of(&sub_representation(&[GRID_A, GRID_A]), &Store)
        .expect("the read succeeds")
        .expect("a system borrowed from the support");
    assert_eq!(read.projected.epsg(), Some(23031));
}

#[test]
fn two_patches_on_two_systems_borrow_nothing() {
    // The same bound as for `Grid`: never the first patch.
    // To break it: return `agreed` instead of `None` at the first disagreement.
    let read = crs::of(&sub_representation(&[GRID_A, GRID_B]), &Store).expect("the read succeeds");
    assert!(read.is_none(), "two supports in two systems: {read:?}");
}
