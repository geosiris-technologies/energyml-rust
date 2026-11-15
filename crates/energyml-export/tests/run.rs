//! The export run: skip or refuse is the request's word, never a face's.

use energyml_export::run::{OnUnhandled, Sink, file_name, run};
use energyml_export::{Exporters, Options};
use energyml_geometry::{Geometry, Patch, SceneObject, Topology};

fn object(uuid: &str, with_points: bool) -> SceneObject {
    SceneObject {
        uuid: uuid.into(),
        uri: format!("eml:///resqml22.TriangulatedSetRepresentation({uuid})"),
        title: uuid.into(),
        qualified_type: "resqml22.TriangulatedSetRepresentation".into(),
        geometry: Geometry {
            kind: "test",
            patches: if with_points {
                vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    topology: Topology::Polylines(vec![3]),
                    closed: Vec::new(),
                }]
            } else {
                Vec::new()
            },
        },
        crs: None,
        interpretation: None,
        feature: None,
        metadata: Default::default(),
        attributes: Vec::new(),
    }
}

#[test]
fn skip_names_what_it_left_out_and_writes_the_rest() {
    let exporters = Exporters::builtin();
    let exporter = exporters.get("obj").expect("obj is registered");
    let objects = vec![object("u-1", true), object("u-2", false)];
    let mut out = Vec::new();
    let report = run(
        &objects,
        exporter,
        &Options::default(),
        OnUnhandled::Skip,
        Sink::Stream(&mut out),
    )
    .expect("skip does not fail");
    assert_eq!(report.written, 1);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].uuid, "u-2");
    assert!(!out.is_empty());
    // Break it: make `run` drop the unhandled object without pushing it to `skipped`.
}

#[test]
fn refuse_fails_naming_the_object_and_writes_nothing() {
    let exporters = Exporters::builtin();
    let exporter = exporters.get("obj").expect("obj is registered");
    let objects = vec![object("u-1", true), object("u-2", false)];
    let mut out = Vec::new();
    let error = run(
        &objects,
        exporter,
        &Options::default(),
        OnUnhandled::Refuse,
        Sink::Stream(&mut out),
    )
    .expect_err("refuse fails");
    assert!(format!("{error}").contains("u-2"), "{error}");
    assert!(out.is_empty(), "nothing is written before the refusal");
    // Break it: let `Refuse` fall through to the skip branch.
}

#[test]
fn a_file_name_carries_the_release_and_the_uuid() {
    let exporters = Exporters::builtin();
    let exporter = exporters.get("obj").expect("obj is registered");
    assert_eq!(
        file_name(&object("u-1", true), exporter),
        "resqml22.TriangulatedSetRepresentation_u-1.obj"
    );
    // Break it: format the name without the uuid.
}
