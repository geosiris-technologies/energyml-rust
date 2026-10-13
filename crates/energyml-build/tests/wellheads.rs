//! `pointset-from-wellheads` - the DEM of a batch of boreholes.
//!
//! The shape of the assertions comes from the rule that a trajectory starts at the
//! first stated depth, so the node at MD 0 is the datum; the values are written here.

use energyml_build::operation::{Choices, Identity, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-22T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A named frame of the package.
fn crs(uuid: &str, title: &str) -> Ref {
    Ref::new(uuid, "eml23.LocalEngineeringCompoundCrs", title)
}

/// A borehole of the package: its shape (trajectory nodes) and what is
/// known of it - including **its collar, which is not its first node**.
fn well(bag: Pieces, name: &str, nodes: &[[f64; 3]], head: Option<[f64; 3]>, frame: Ref) -> Pieces {
    let geometry = Geometry {
        kind: "resqml22.WellboreTrajectoryRepresentation",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points: nodes.to_vec(),
            topology: Topology::Polylines(vec![nodes.len() as u32]),
            closed: Vec::new(),
        }],
    };
    bag.with(
        name,
        Source::Shape {
            geometry,
            closed: None,
        },
    )
    .with(
        format!("{name} (object)"),
        Source::Object(Known {
            reference: Some(Ref::new(
                format!("uuid-{name}"),
                "resqml22.WellboreTrajectoryRepresentation",
                name,
            )),
            crs: Some(frame),
            // The codes the wells' frame states: the collars, read placed, are
            // written under the identity compound of these.
            crs_epsg: Some(2154),
            vertical_epsg: Some(5720),
            wellhead: head,
            ..Known::default()
        }),
    )
}

fn plan(pieces: &Pieces) -> Result<energyml_build::operation::Plan, String> {
    Registry::new()
        .plan(
            "pointset-from-wellheads",
            pieces,
            // The frame is **named by a reference**, which a `crs` slot
            // expects: a string would be a title, and the module refuses it.
            &Choices::new()
                .set("crs", crs("c", "the frame").dor())
                .set("title", "MNT"),
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.to_string())
}

/// The scatter that the plan writes.
fn points(made: &energyml_build::operation::Plan) -> Vec<[f64; 3]> {
    let set = made
        .datasets
        .iter()
        .find(|set| set.path.ends_with("points"))
        .expect("the points");
    let energyml_build::operation::Numbers::F64(values) = &set.values else {
        panic!("doubles")
    };
    values.chunks(3).map(|p| [p[0], p[1], p[2]]).collect()
}

#[test]
fn the_collar_comes_from_the_datum_and_not_from_the_first_node() {
    // **The check that carries the whole operation.** Since 22/09 a
    // trajectory starts at the first stated depth, so its first
    // node is **not** the ground: on a borehole whose log starts at 31.5 m,
    // reading it there would answer thirty metres too low - and a terrain
    // surface thirty metres too low looks exactly like a terrain surface.
    //
    // The two boreholes below are made for that: their first node is
    // far below their collar, and by a different distance for each. An
    // implementation that read the geometry would return `[…, 88.5]` and
    // `[…, 119.0]`, which are perfectly plausible numbers.
    let bag = well(
        Pieces::empty(),
        "F1",
        &[[0.0, 0.0, 88.5], [0.0, 0.0, 50.0]],
        Some([0.0, 0.0, 120.0]),
        crs("c", "the frame"),
    );
    let bag = well(
        bag,
        "F2",
        &[[10.0, 20.0, 119.0], [10.0, 20.0, 90.0]],
        Some([10.0, 20.0, 131.0]),
        crs("c", "the frame"),
    );

    let made = plan(&bag).expect("a plan");
    assert_eq!(
        points(&made),
        [[0.0, 0.0, 120.0], [10.0, 20.0, 131.0]],
        "the collars, not the first nodes"
    );

    let kinds: Vec<&str> = made.objects.iter().map(|o| o.qualified_type.as_str()).collect();
    assert!(kinds.contains(&"resqml22.PointSetRepresentation"), "{kinds:?}");
}

#[test]
fn a_wellbore_with_no_datum_is_named_rather_than_left_out() {
    // A mute borehole set aside in silence would make a DEM with holes a DEM that
    // looks complete - the result that most looks like a success.
    let bag = well(
        Pieces::empty(),
        "F1",
        &[[0.0, 0.0, 100.0], [0.0, 0.0, 50.0]],
        Some([0.0, 0.0, 120.0]),
        crs("c", "the frame"),
    );
    let bag = well(
        bag,
        "sans-datum",
        &[[1.0, 1.0, 100.0], [1.0, 1.0, 50.0]],
        None,
        crs("c", "the frame"),
    );

    let why = plan(&bag).expect_err("refused");
    assert!(why.contains("sans-datum"), "the mute borehole is named: {why}");
    assert!(why.contains("ReferencePointInACrs"), "and the remedy too: {why}");
}

#[test]
fn two_frames_have_no_common_ground() {
    // Two boreholes written in two systems have no common frame, and
    // putting their collars in one cloud would place them wrong without any
    // number looking abnormal.
    let bag = well(
        Pieces::empty(),
        "F1",
        &[[0.0, 0.0, 100.0], [0.0, 0.0, 50.0]],
        Some([0.0, 0.0, 120.0]),
        crs("c", "Lambert-93"),
    );
    let bag = well(
        bag,
        "F2",
        &[[1.0, 1.0, 100.0], [1.0, 1.0, 50.0]],
        Some([1.0, 1.0, 121.0]),
        crs("d", "RGF93/CC49"),
    );

    let why = plan(&bag).expect_err("refused");
    assert!(
        why.contains("Lambert-93") && why.contains("RGF93/CC49"),
        "both frames are named: {why}"
    );
}

#[test]
fn an_empty_selection_is_refused_on_its_slot() {
    let why = plan(&Pieces::empty()).expect_err("refused");
    assert!(why.contains("wells"), "{why}");
}

#[test]
fn the_same_wells_write_the_same_pointset_twice() {
    // Derived identity: two runs of the same batch write the same package, which
    // is what makes an import replayable.
    let bag = well(
        Pieces::empty(),
        "F1",
        &[[0.0, 0.0, 100.0], [0.0, 0.0, 50.0]],
        Some([0.0, 0.0, 120.0]),
        crs("c", "the frame"),
    );
    let first = plan(&bag).expect("a plan");
    let again = plan(&bag).expect("a plan");
    let uuids = |made: &energyml_build::operation::Plan| -> Vec<String> {
        made.objects.iter().map(|o| o.uuid.clone()).collect()
    };
    assert_eq!(uuids(&first), uuids(&again));
}

/// **The collars are written under a frame where placed is local, and the plan
/// says so** - or refused by name when the wells' frame states no projected
/// code. `Known::wellhead` is a *placed* collar (projected x, y, an elevation:
/// `read::wellhead_of`), so the wells' own compound, whose origin and azimuth a
/// pure operation does not receive, cannot hold it as it stands.
#[test]
fn placed_collars_are_written_under_the_identity_frame_of_the_wells_codes() {
    let bag = well(
        Pieces::empty(),
        "F1",
        &[[0.0, 0.0, 100.0], [0.0, 0.0, 50.0]],
        Some([0.0, 0.0, 120.0]),
        crs("c", "the frame"),
    );
    let made = plan(&bag).expect("a plan");
    let frame = made
        .of_role("crs")
        .next()
        .expect("the frame the collars are written in");
    assert_eq!(frame.qualified_type, "eml23.LocalEngineeringCompoundCrs");
    assert_ne!(frame.uuid, "c", "not the wells' own compound");
    let note = made
        .says
        .iter()
        .find(|n| n.key == "collars-placed")
        .unwrap_or_else(|| panic!("{:?}", made.says));
    assert!(
        note.detail.contains("EPSG:2154")
            && note.detail.contains("EPSG:5720")
            && note.detail.contains("the frame"),
        "{}",
        note.detail
    );
    let written = made.of_role("representation").next().unwrap();
    assert_eq!(
        written.json["NodePatchGeometry"][0]["LocalCrs"]["Uuid"],
        serde_json::json!(frame.uuid)
    );

    // No projected code stated: no system to write placed numbers in.
    let mute = Pieces::empty()
        .with(
            "F1",
            Source::Shape {
                geometry: Geometry {
                    kind: "well",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points: vec![[0.0, 0.0, 100.0], [0.0, 0.0, 50.0]],
                        topology: Topology::Polylines(vec![2]),
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        )
        .with(
            "F1 (object)",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "uuid-F1",
                    "resqml22.WellboreTrajectoryRepresentation",
                    "F1",
                )),
                crs: Some(crs("c", "a frame with no code")),
                wellhead: Some([0.0, 0.0, 120.0]),
                ..Known::default()
            }),
        );
    let why = plan(&mute).expect_err("no system");
    assert!(
        why.contains("a frame with no code") && why.contains("sourceEpsg"),
        "{why}"
    );
}

/// **Under a turned frame, the collar is written where the datum is** - A defect of
/// an audit: the collars were written with
/// the datums' coordinates as they stand, under the wells' `LocalCrs`. Right
/// under an identity compound, which is every package of the corpus; wrong as
/// soon as the compound has an origin or an azimuth - the grid frame of
/// Lutetia is turned 30°.
///
/// The whole way, and two readers: a frame turned by `crs`, a well written in
/// it by `well-from-polyline`, the package read back by `read::pieces_of_object` -
/// which places `Known::wellhead` through the compound its datum names - then
/// the collar written here, under the identity compound of the same codes,
/// against the datum read by `energyml_geometry::well`. To break: the collar
/// written under the turned compound, where its placed numbers would be read
/// as local ones and turned a second time.
#[cfg(feature = "hdf5")]
#[test]
fn under_a_turned_frame_a_collar_is_written_where_its_datum_is() {
    use std::sync::Arc;
    let dir = std::env::temp_dir().join(format!("energyml-build-wellheads-turned-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("turned.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "turned.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);

    let frame_plan = Registry::new()
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new()
                .set("epsg", 2154)
                .set("vertical", 5720)
                .set("originEasting", 651_000.0)
                .set("originNorthing", 6_862_000.0)
                .set("azimuth", 30.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert!(
        energyml_build::apply::apply(&frame_plan, &mut store, &when())
            .refused
            .is_empty()
    );
    let compound = frame_plan.of_role("crs").next().expect("the compound");
    let frame = Ref::new(
        compound.uuid.clone(),
        compound.qualified_type.clone(),
        compound.title.clone(),
    );

    // A well collared at the local (10, 20), 50 up.
    let line = vec![[10.0, 20.0, 50.0], [10.0, 20.0, -50.0], [40.0, 60.0, -100.0]];
    let pieces = Pieces::empty()
        .with(
            "path",
            Source::Shape {
                geometry: Geometry {
                    kind: "polyline",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points: line,
                        topology: Topology::Polylines(vec![3]),
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        )
        .with(
            "path (object)",
            Source::Object(Known {
                crs: Some(frame),
                vertical_up: Some(true),
                vertical_uom: Some("m".into()),
                ..Known::default()
            }),
        );
    let well = Registry::new()
        .plan(
            "well-from-polyline",
            &pieces,
            &Choices::new().set("ground", 50.0).set("title", "F1"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert!(
        energyml_build::apply::apply(&well, &mut store, &when())
            .refused
            .is_empty()
    );
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();

    let reread = energyml_storage::EpcStore::open(&out, registry).unwrap();
    let trajectory_uuid = well.of_role("trajectory").next().unwrap().uuid.clone();
    let read =
        energyml_build::read::pieces_of_object(&reread, &trajectory_uuid).expect("the trajectory reads");
    let mut wells = Pieces::empty();
    for (name, source) in energyml_build::read::in_slot("wells", read) {
        wells = wells.with(name, source);
    }
    let made = Registry::new()
        .plan(
            "pointset-from-wellheads",
            &wells,
            &Choices::new().set("title", "MNT"),
            &Identity::derived(),
            &when(),
        )
        .expect("a plan");
    // Not under the turned compound: under the identity one of 2154 + 5720,
    // where the placed collar is a local number.
    let written = made.of_role("representation").next().expect("the point set");
    let frame = made.of_role("crs").next().expect("the frame written");
    assert_ne!(
        frame.uuid, compound.uuid,
        "the turned compound cannot hold a placed collar"
    );
    assert_eq!(
        written.json["NodePatchGeometry"][0]["LocalCrs"]["Uuid"],
        serde_json::json!(frame.uuid)
    );
    assert_eq!(frame.json["OriginVerticalCoordinate"], serde_json::json!(0.0));
    let collar = points(&made)[0];

    let summary = reread.find_by_uuid(&trajectory_uuid).cloned().unwrap();
    let trajectory = reread.get_object(&summary.uri).unwrap();
    let datum = energyml_geometry::well::head_of(trajectory.as_reflect(), &reread).expect("the datum reads");
    for axis in 0..3 {
        assert!(
            (collar[axis] - datum[axis]).abs() < 1e-6,
            "axis {axis}: the collar is written at {collar:?}, the datum is at {datum:?}"
        );
    }
    // And the turned frame did move it - the local collar is (10, 20): the
    // agreement would say nothing of that defect if both were the raw numbers.
    assert!(
        (collar[0] - 10.0).abs() > 1_000.0 && (collar[1] - 20.0).abs() > 1_000.0,
        "{collar:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
