//! A well whose depth bounds sit **flat on the trajectory**.
//!
//! RESQML 2.0.1 and the 2.2 development drops put `StartMd`, `FinishMd`,
//! `MdUom` and `MdDatum` directly on the `WellboreTrajectoryRepresentation`;
//! the released 2.2 groups the same four values into an `MdInterval`. The
//! datum differs too — `MdDatum` against `ReferencePointInACrs` — and so does
//! how it carries its position and its vertical sense:
//!
//! | | 2.0.1 and the drops | released 2.2 |
//! |---|---|---|
//! | bounds | `StartMd`, `FinishMd`, `MdUom`, `MdDatum` | `MdInterval{MdMin, MdMax, Uom, Datum}` |
//! | datum type | `MdDatum` | `ReferencePointInACrs` |
//! | position | `Location.Point3d{C1,C2,C3}` | `HorizontalCoordinates` + `VerticalCoordinate` |
//! | vertical sense | `Location.LocalCrs.ZIncreasingDownward` | `VerticalCrs` (EPSG 5714/5715, `Direction`) |
//!
//! `tests/well.rs` covers the right-hand column. This file covers the left,
//! which nothing did: the corpus held no trajectory of that shape without a
//! `Geometry`, so 186 wells of a client package refused with *"neither
//! `Geometry` nor `MdInterval`"* and their 152 frames fell with them.

use energyml_core::registry::DynObject;
use energyml_domain::source::ObjectSource;
use energyml_eml_2_2::{DataObjectReference, LengthUomExt};
use energyml_geometry::{Topology, geometry};
use energyml_resqml_2_2_dev3::{
    LocalDepth3dCrs, MdDatum, Point3d, SinglePointGeometry, WellboreTrajectoryRepresentation,
};

const DATUM: &str = "11111111-1111-1111-1111-111111111111";
const LOCAL_CRS: &str = "22222222-2222-2222-2222-222222222222";

fn dor(uuid: &str) -> DataObjectReference {
    DataObjectReference {
        uuid: uuid.to_string(),
        ..Default::default()
    }
}

/// The Guadeloupe shape: a wellhead at (656719.86, 1794344.31), its third
/// coordinate an **elevation** because the local CRS says z increases upward.
fn datum() -> MdDatum {
    MdDatum {
        uuid: DATUM.to_string(),
        location: SinglePointGeometry {
            local_crs: dor(LOCAL_CRS),
            point3d: Point3d {
                coordinate1: 656_719.86,
                coordinate2: 1_794_344.31,
                coordinate3: -6.85,
            },
            ..Default::default()
        },
        ..Default::default()
    }
}

/// `ZIncreasingDownward = false`, so the datum's third coordinate is a height.
fn local_crs() -> LocalDepth3dCrs {
    LocalDepth3dCrs {
        uuid: LOCAL_CRS.to_string(),
        z_increasing_downward: false,
        projected_uom: Default::default(),
        vertical_uom: Default::default(),
        ..Default::default()
    }
}

fn trajectory() -> WellboreTrajectoryRepresentation {
    WellboreTrajectoryRepresentation {
        start_md: 0.0,
        finish_md: 34.0,
        // Stated, never defaulted: `LengthUomExt`'s own default is a tenth of
        // a foot, so leaving it out converted this well's 34 m into 1.04 m
        // and drew a plausible, shallow, wrong hole.
        md_uom: LengthUomExt::M,
        md_datum: dor(DATUM),
        ..Default::default()
    }
}

struct Wellsite;

impl ObjectSource for Wellsite {
    fn object(&self, uuid: &str) -> Option<Box<dyn DynObject>> {
        match uuid {
            DATUM => Some(Box::new(datum())),
            LOCAL_CRS => Some(Box::new(local_crs())),
            _ => None,
        }
    }
}

impl energyml_domain::source::ArraySource for Wellsite {
    fn locate(
        &self,
        _object: &dyn energyml_core::value::Reflect,
    ) -> Vec<energyml_storage::ArrayLocation> {
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

/// The flat bounds are read, and the well stands from its datum downward.
#[test]
fn a_trajectory_with_flat_bounds_is_a_vertical_well() {
    let found = geometry::of(&trajectory(), &Wellsite).expect("the drop shape is read");
    assert_eq!(found.patches.len(), 1);
    let patch = &found.patches[0];
    assert!(matches!(&patch.topology, Topology::Polylines(runs) if runs == &vec![2]));

    // Head at the datum's own elevation, bottom 34 m below it. Reading
    // `ZIncreasingDownward = false` the other way would put the bottom *above*
    // the head, which is the kind of wrong that still draws a well.
    assert_eq!(patch.points[0], [656_719.86, 1_794_344.31, -6.85]);
    assert_eq!(patch.points[1], [656_719.86, 1_794_344.31, -6.85 - 34.0]);
}

/// The CRS comes through the datum here too — and it is a **whole** one.
///
/// An `MdDatum` names a `LocalDepth3dCrs`, which carries offsets, EPSG code and
/// vertical direction together; the 2.2 datum has no such object and its CRS
/// has to be assembled from a horizontal and a vertical half. Reading the whole
/// one is what keeps a package with non-zero offsets from putting its wells
/// where its surfaces are not — with every individual number still looking
/// reasonable.
#[test]
fn the_flat_datum_gives_the_well_a_whole_crs() {
    let crs = energyml_domain::crs::of(&trajectory(), &Wellsite)
        .expect("no error")
        .expect("a CRS is found through the datum");
    assert_eq!(
        crs.vertical_direction,
        energyml_domain::VerticalDirection::Up,
        "`ZIncreasingDownward = false` means the third coordinate is a height"
    );
}

/// A trajectory with nothing to place it by is still refused, by name.
///
/// The fallback added for the flat shape reads the trajectory *itself* when
/// there is no `MdInterval`, and that widens what looks like an answer: in this
/// release `StartMd` and `FinishMd` are required, so an empty object carries
/// `0.0` for both and the bounds check passes on values nobody wrote. What
/// stops it there is the datum — which the specification's own business rule
/// makes mandatory — so the refusal names that rather than the bounds. The
/// point of the test is that it refuses at all: a well silently placed at the
/// origin with zero length is the outcome to rule out.
#[test]
fn a_trajectory_with_nothing_to_place_it_by_is_still_refused() {
    let empty = WellboreTrajectoryRepresentation::default();
    let error = geometry::of(&empty, &Wellsite).expect_err("nothing to place it by");
    let said = error.to_string();
    assert!(
        said.contains("datum"),
        "the refusal says what was missing: {said}"
    );
}
