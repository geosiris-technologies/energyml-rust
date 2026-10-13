//! `PlaneSetRepresentation` — the one that is drawn, and the one that is refused.
//!
//! **Nothing in the corpus holds one**, which is why it had no reader until
//! 05/09. So the objects are built here, and the rule of this workspace applies
//! in its strict form: *the shape of the assertions comes from the
//! specification, the values from the file* — with no file, both come from the
//! schema, and the schema is quoted where it decides something.
//!
//! Two facts are worth a test each, and they are the two ways this reader could
//! be wrong without failing:
//!
//! - a plane set of three planes drawn as two, because one of them was silently
//!   skipped. A picture that looks complete and is not is this crate's whole
//!   subject;
//! - the second triple's triangle indexed `0,1,2` like the first, which draws
//!   the first plane twice and the second nowhere — with the right number of
//!   points, the right number of triangles, and nothing to see.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::source::{ArraySource, ObjectSource};
use energyml_geometry::{Topology, geometry};
use energyml_resqml_2_2::{
    AbstractPlaneGeometry, HorizontalPlaneGeometry, PlaneSetRepresentation, Point3d, ThreePoint3d,
    TiltedPlaneGeometry,
};
use energyml_storage::array::{ArrayBuffer, ArrayData, ArrayLocation, ArrayMetadata};

/// A store with nothing in it.
///
/// A plane set keeps its numbers in the XML — nine per triple — so a reader
/// that reached for an array would fail here rather than quietly read one. That
/// is the point of handing it an empty store rather than a working one.
struct NoStore;

impl ObjectSource for NoStore {
    fn object(&self, _uuid: &str) -> Option<Box<dyn DynObject>> {
        None
    }
}

impl ArraySource for NoStore {
    fn locate(&self, _object: &dyn Reflect) -> Vec<ArrayLocation> {
        Vec::new()
    }

    fn metadata(&self, _location: &ArrayLocation) -> energyml_domain::Result<ArrayMetadata> {
        panic!("a plane set must not reach for an array")
    }

    fn read(&self, _location: &ArrayLocation) -> energyml_domain::Result<ArrayData> {
        panic!("a plane set must not reach for an array")
    }

    fn read_into(
        &self,
        _location: &ArrayLocation,
        _into: &mut ArrayBuffer<'_>,
    ) -> energyml_domain::Result<ArrayMetadata> {
        panic!("a plane set must not reach for an array")
    }
}

fn point(x: f64, y: f64, z: f64) -> Point3d {
    Point3d {
        coordinate1: x,
        coordinate2: y,
        coordinate3: z,
    }
}

/// One triple, at coordinates no two of which share a value.
///
/// Every number distinct, so a reader that transposed the axes, repeated a
/// point or read the wrong corner gets a different answer rather than the same
/// one by luck.
fn triple(at: f64) -> ThreePoint3d {
    ThreePoint3d {
        point3d: vec![
            point(at, at + 1.0, at + 2.0),
            point(at + 3.0, at + 4.0, at + 5.0),
            point(at + 6.0, at + 7.0, at + 8.0),
        ],
    }
}

fn tilted(triples: Vec<ThreePoint3d>) -> AbstractPlaneGeometry {
    AbstractPlaneGeometry::TiltedPlaneGeometry(TiltedPlaneGeometry {
        plane: triples,
        ..Default::default()
    })
}

fn set(planes: Vec<AbstractPlaneGeometry>) -> PlaneSetRepresentation {
    PlaneSetRepresentation {
        planes,
        ..Default::default()
    }
}

#[test]
fn a_tilted_plane_is_the_triangle_its_three_points_span() {
    // The nine numbers are locations the file states; the face through them is
    // not an interpretation of the plane, it is those three points. Nothing
    // here is invented, which is why this form is drawn at all.
    let found = geometry::of(&set(vec![tilted(vec![triple(0.0)])]), &NoStore).expect("it evaluates");

    assert_eq!(found.patches.len(), 1);
    let patch = &found.patches[0];
    assert_eq!(
        patch.points,
        vec![[0.0, 1.0, 2.0], [3.0, 4.0, 5.0], [6.0, 7.0, 8.0]]
    );
    assert_eq!(patch.topology, Topology::Triangles(vec![[0, 1, 2]]));
}

#[test]
fn a_second_triple_is_indexed_from_its_own_points() {
    // **The mistake this guards is invisible in every count.** Indexing the
    // second triangle `0,1,2` like the first gives three points, two triangles
    // and the right patch — and draws the first plane twice while the second
    // appears nowhere.
    let found =
        geometry::of(&set(vec![tilted(vec![triple(0.0), triple(100.0)])]), &NoStore).expect("it evaluates");

    let patch = &found.patches[0];
    assert_eq!(patch.points.len(), 6);
    assert_eq!(patch.points[3], [100.0, 101.0, 102.0]);
    assert_eq!(patch.topology, Topology::Triangles(vec![[0, 1, 2], [3, 4, 5]]));
}

#[test]
fn each_plane_of_the_set_is_a_patch_of_its_own() {
    // A set of three drawn as two is the shape of failure this crate exists to
    // refuse: it looks complete. The node offsets are checked too, because
    // anything indexing *nodes of the representation* — a property, a
    // sub-representation — counts from them.
    let found = geometry::of(
        &set(vec![
            tilted(vec![triple(0.0)]),
            tilted(vec![triple(10.0)]),
            tilted(vec![triple(20.0)]),
        ]),
        &NoStore,
    )
    .expect("it evaluates");

    assert_eq!(found.patches.len(), 3);
    assert_eq!(found.point_count(), 9);
    assert_eq!(
        found.patches.iter().map(|p| p.node_offset).collect::<Vec<_>>(),
        vec![0, 3, 6]
    );
}

#[test]
fn a_horizontal_plane_is_refused_by_name_and_says_what_it_does_hold() {
    // It carries `Coordinate`: one number, a depth. A plane at a depth is
    // infinite, and no release will add an extent — the extent is a decision
    // about the picture, not a fact about the model. Skipping it instead would
    // draw a set of two planes as one.
    let error = geometry::of(
        &set(vec![
            tilted(vec![triple(0.0)]),
            AbstractPlaneGeometry::HorizontalPlaneGeometry(HorizontalPlaneGeometry {
                coordinate: -1500.0,
                ..Default::default()
            }),
        ]),
        &NoStore,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("HorizontalPlaneGeometry"), "{error}");
    assert!(error.contains("no extent of its own"), "{error}");
    // And it says which one, so a set of forty can be acted on.
    assert!(error.contains("Planes[1]"), "{error}");
}

#[test]
fn a_triple_that_is_not_three_points_is_malformed() {
    // `min = 3, max = 3` in the schema. Reading the first three of a longer
    // list, or padding a shorter one, would draw a triangle the file does not
    // state.
    let error = geometry::of(
        &set(vec![tilted(vec![ThreePoint3d {
            point3d: vec![point(0.0, 0.0, 0.0), point(1.0, 1.0, 1.0)],
        }])]),
        &NoStore,
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("exactly three points"), "{error}");
    assert!(error.contains("states 2"), "{error}");
}

#[test]
fn a_set_with_no_plane_at_all_is_refused() {
    // `min = 1` in the schema, and an empty vector of patches is a successful
    // call with nothing to draw — the one answer this crate never gives.
    let error = geometry::of(&set(Vec::new()), &NoStore).unwrap_err().to_string();
    assert!(error.contains("no plane at all"), "{error}");
}

#[test]
fn the_reader_is_registered_under_the_concept() {
    // Keyed on `type_key`, so the day RESQML 2.3 keeps the concept this reader
    // covers it without being touched.
    assert!(geometry::is_supported(&set(vec![tilted(vec![triple(0.0)])])));
}
