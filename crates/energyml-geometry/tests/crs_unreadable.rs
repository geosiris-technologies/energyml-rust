//! An object whose CRS was named but could not be read is not an object with no
//! CRS (`[geometry G4]`): metres are refused by name, local and projected
//! coordinates still come back, and the reason travels in the metadata.

use energyml_geometry::geometry::{Geometry, Patch, Topology};
use energyml_geometry::scene::{CRS_UNREADABLE, Placement, SceneObject};

fn object(unreadable: Option<&str>) -> SceneObject {
    let mut metadata = energyml_domain::metadata::Metadata::default();
    if let Some(why) = unreadable {
        metadata.insert(CRS_UNREADABLE, why);
    }
    SceneObject {
        uuid: "cccccccc-cccc-4ccc-8ccc-cccccccccccc".to_string(),
        uri: "eml:///resqml22.PointSetRepresentation(cccccccc-cccc-4ccc-8ccc-cccccccccccc)".to_string(),
        title: String::new(),
        qualified_type: "resqml22.PointSetRepresentation".to_string(),
        geometry: Geometry {
            kind: "PointSetRepresentation",
            patches: vec![Patch {
                index: 0,
                node_offset: 0,
                points: vec![[1.0, 2.0, 3.0]],
                topology: Topology::Points,
                closed: Vec::new(),
            }],
        },
        crs: None,
        interpretation: None,
        feature: None,
        metadata,
        attributes: Vec::new(),
    }
}

/// Break it: remove the `CRS_UNREADABLE` test in `SceneObject::place` and the
/// metres request answers the local points again.
#[test]
fn metres_are_refused_when_the_named_crs_could_not_be_read() {
    let refused = object(Some("the CRS aaaa is missing"))
        .points_in(Placement::Metres)
        .expect_err("metres of an object whose CRS is unreadable");
    let message = refused.to_string();
    assert!(
        message.contains("cannot be read") && message.contains("the CRS aaaa is missing"),
        "{message}"
    );
    // Local coordinates were asked for nothing placed: they are what is returned.
    assert_eq!(
        object(Some("the CRS aaaa is missing"))
            .points_in(Placement::Local)
            .unwrap(),
        vec![[1.0, 2.0, 3.0]]
    );
    // An object that names no CRS is the legal case and is unchanged.
    assert_eq!(
        object(None).points_in(Placement::Metres).unwrap(),
        vec![[1.0, 2.0, 3.0]]
    );
}
