//! The three representation operations - `pointset`, `polylines`,
//! `triangles` - on the two kinds of input they take: a shape already in a
//! frame of the package, and a feature collection in its own system.
//!
//! What would have to break for each to go red is said on each.

use serde_json::json;

use energyml_build::geojson;
use energyml_build::operation::{Choices, Fitness, Identity, Registry, When};
use energyml_build::pieces::{Known, Lattice, Pieces, Ref, Source};
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When {
        creation: "2026-09-09T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// The frame the shapes of these tests are in - a **compound**.
///
/// It was a `LocalEngineering2dCrs` until 24/09, and every test here took it
/// as a frame: a half of a system, with no vertical, named as a geometry's
/// `LocalCrs`. RESQML TRG 2.2 §3.1 p. 22 associates the compound; since
/// 24/09 `support::frame_in` blocks the writing of a half by its name, and
/// `a_half_of_a_system_named_as_the_frame_blocks_the_writing_by_its_name`
/// holds it.
fn crs_ref() -> Ref {
    Ref::new(
        "aaaaaaaa-0000-4000-8000-000000000001",
        "eml23.LocalEngineeringCompoundCrs",
        "The frame",
    )
}

/// A shape of the package: its geometry, and what the caller knows of it.
fn known(geometry: Geometry) -> Pieces {
    Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry,
                closed: None,
            },
        )
        .with(
            "object",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml22.PolylineSetRepresentation",
                    "The source",
                )),
                crs: Some(crs_ref()),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        )
}

fn patch(index: usize, points: Vec<[f64; 3]>, topology: Topology) -> Patch {
    Patch {
        index,
        node_offset: 0,
        points,
        topology,
        closed: Vec::new(),
    }
}

fn square() -> Vec<[f64; 3]> {
    vec![
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
    ]
}

fn lines_geojson() -> String {
    r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","geometry":{"type":"LineString","coordinates":[[2.0,48.0,10],[2.1,48.0,11],[2.2,48.1,12]]},"properties":{}},
      {"type":"Feature","geometry":{"type":"Polygon","coordinates":[[[2.0,48.0],[2.1,48.0],[2.1,48.1],[2.0,48.1],[2.0,48.0]]]},"properties":{}}
    ]}"#
    .to_string()
}

// -- pointset ---------------------------------------------------------------

#[test]
fn a_shape_in_a_frame_of_the_package_becomes_a_point_set_in_that_frame() {
    // To break: the frame taken from `sourceEpsg` when a `crs` was suggested,
    // or the points moved on the way.
    let pieces = known(Geometry {
        kind: "polylines",
        patches: vec![patch(0, square(), Topology::Polylines(vec![4]))],
    });
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "pointset")
        .unwrap();
    assert_eq!(offer.fitness, Fitness::Fit);
    let crs = offer.needs.iter().find(|s| s.name == "crs").unwrap();
    assert_eq!(
        crs.from
            .as_ref()
            .and_then(energyml_build::operation::Provenance::reference)
            .map(|r| r.uuid.as_str()),
        Some("aaaaaaaa-0000-4000-8000-000000000001")
    );
    let title = offer.needs.iter().find(|s| s.name == "title").unwrap();
    assert_eq!(title.suggested, Some(json!("shape")));

    // Everything by suggestion: the crs, the title, the data file.
    let plan = registry
        .plan(
            "pointset",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(plan.objects.len(), 1, "no CRS created: one was named");
    let object = &plan.objects[0];
    assert_eq!(object.qualified_type, "resqml22.PointSetRepresentation");
    assert_eq!(
        object.json["NodePatchGeometry"][0]["LocalCrs"]["Uuid"],
        json!("aaaaaaaa-0000-4000-8000-000000000001")
    );
    assert_eq!(plan.datasets.len(), 1);
    assert_eq!(plan.datasets[0].dimensions, vec![4, 3]);
    assert_eq!(plan.datasets[0].path, format!("/resqml22/{}/points", object.uuid));
    assert_eq!(
        object.json["NodePatchGeometry"][0]["Points"]["Coordinates"]["ExternalDataArrayPart"][0]["Count"],
        json!([4, 3])
    );
}

#[test]
fn a_feature_collection_becomes_a_point_set_with_its_own_crs_created() {
    // To break: a GeoJSON silently placed in the package's frame, or the CRS
    // objects missing from the plan while the geometry references them.
    let collection = geojson::read(&lines_geojson()).unwrap();
    let pieces = Pieces::empty().with(
        "lines.geojson",
        Source::Features {
            name: "lines.geojson".into(),
            features: collection.features,
        },
    );
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "pointset")
        .unwrap();
    assert_eq!(
        offer
            .needs
            .iter()
            .find(|s| s.name == "sourceEpsg")
            .unwrap()
            .suggested,
        Some(json!(4326))
    );

    let plan = registry
        .plan(
            "pointset",
            &pieces,
            &Choices::new().set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    // **The frame created is the compound over the projected system, and it is
    // what the geometry names.** Until 24/09 the roles were `crs`,
    // `vertical-crs`, `representation`, and `crs` was the `ProjectedCrs` -
    // which this test pinned as the `LocalCrs`, against RESQML TRG 2.2 §3.1
    // p. 22, the vertical system referenced by nothing.
    assert_eq!(
        roles,
        vec![
            "projected-crs",
            "vertical-crs",
            "local-2d-crs",
            "crs",
            "representation"
        ]
    );
    assert_eq!(plan.objects[0].qualified_type, "eml23.ProjectedCrs");
    let frame = plan.of_role("crs").next().expect("the frame");
    assert_eq!(frame.qualified_type, "eml23.LocalEngineeringCompoundCrs");
    let representation = plan.of_role("representation").next().expect("the point set");
    let geometry = &representation.json["NodePatchGeometry"][0];
    assert_eq!(geometry["LocalCrs"]["Uuid"], json!(frame.uuid));
    assert_eq!(
        geometry["LocalCrs"]["QualifiedType"],
        json!("eml23.LocalEngineeringCompoundCrs")
    );
    // 3 + 5 positions, the ring's repeat included - a point set keeps every position.
    assert_eq!(plan.datasets[0].dimensions, vec![8, 3]);
    // Placed in metres: a Lambert-93 easting is six hundred thousand-odd.
    let x = match &plan.datasets[0].values {
        energyml_build::operation::Numbers::F64(v) => v[0],
        _ => panic!("floats"),
    };
    assert!(x > 500_000.0 && x < 800_000.0, "{x}");
}

#[test]
fn a_feature_collection_against_a_crs_of_the_package_is_refused() {
    // Its numbers are in its own system; the package's frame cannot be assumed.
    let collection = geojson::read(&lines_geojson()).unwrap();
    let pieces = Pieces::empty().with(
        "lines.geojson",
        Source::Features {
            name: "lines.geojson".into(),
            features: collection.features,
        },
    );
    let why = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new().set("crs", crs_ref().dor()),
            &Identity::derived(),
            &when(),
        )
        .expect_err("refused");
    assert!(why.message().contains("own system"), "{why}");
}

/// **This test said "refused" until 12/09.** A missing frame became what a wrong release already was:
/// a condition of the **writing**, carried in `blocked`, never a refusal to
/// compute. The shape comes out, the object is there with a stand-in reference,
/// and `apply` is the only one to refuse - naming the two ways of giving a
/// frame.
#[test]
fn a_shape_without_a_frame_is_blocked_by_name_not_refused() {
    let pieces = Pieces::empty().with(
        "shape",
        Source::Shape {
            geometry: Geometry {
                kind: "obj",
                patches: vec![patch(0, square(), Topology::Points)],
            },
            closed: None,
        },
    );
    let plan = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new().set("title", "x"),
            &Identity::derived(),
            &when(),
        )
        .expect("the geometry does not need a frame");
    assert_eq!(plan.shapes.len(), 1);
    let why = plan.blocked.as_ref().expect("the writing does");
    assert!(
        why.message().contains("`crs`") && why.message().contains("`sourceEpsg`"),
        "{why}"
    );
}

/// **A half of a system named as the frame blocks the writing, by its name.**
///
/// A frame must be a whole system: `frame_in` took **any** reference, so a geometry
/// could name a `VerticalCrs` - or a `LocalEngineering2dCrs`, which these very
/// tests did - as its `LocalCrs`. Each half reads back perfectly well and
/// places nothing whole: no horizontal, or no vertical. RESQML TRG 2.2 §3.1
/// p. 22 associates the compound. Carried and not raised, like a CRS of
/// another release: the geometry does not depend on the frame.
///
/// To break: `support::frame_in` taking the reference without
/// `crs::frame_kind`.
#[test]
fn a_half_of_a_system_named_as_the_frame_blocks_the_writing_by_its_name() {
    let pieces = known(Geometry {
        kind: "x",
        patches: vec![patch(0, square(), Topology::Points)],
    });
    for (half, what) in [
        ("eml23.VerticalCrs", "vertical half"),
        ("eml23.LocalEngineering2dCrs", "horizontal half"),
        ("eml23.ReferencePointInACrs", "reference point"),
    ] {
        let named = Ref::new("dddddddd-0000-4000-8000-000000000004", half, "A half").dor();
        let plan = Registry::new()
            .plan(
                "pointset",
                &pieces,
                &Choices::new().set("crs", named).set("title", "x"),
                &Identity::derived(),
                &when(),
            )
            .expect("the geometry does not need a frame");
        assert!(!plan.shapes.is_empty(), "{half}: the shape is computed");
        let why = plan
            .blocked
            .as_ref()
            .unwrap_or_else(|| panic!("{half} is no frame"))
            .to_string();
        assert!(why.contains(half) && why.contains(what), "{half}: {why}");
    }
    // And the control this needs: the compound of the same bag blocks nothing,
    // so it is the type that blocks, not the bag.
    let whole = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new().set("title", "x"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert!(whole.blocked.is_none(), "{:?}", whole.blocked);
}

/// **A `ProjectedCrs` named as it stands is taken, and said** - a deliberate
/// decision of 24/09: the BRGM's surfaces point at theirs, and refusing
/// them would make every operation on them a dead end. The note is the
/// difference with a compound, and without it the choice would be silent.
#[test]
fn a_projected_crs_named_as_the_frame_is_taken_with_a_note() {
    let pieces = known(Geometry {
        kind: "x",
        patches: vec![patch(0, square(), Topology::Points)],
    });
    let projected = Ref::new(
        "eeeeeeee-0000-4000-8000-000000000005",
        "eml23.ProjectedCrs",
        "EPSG:2154",
    )
    .dor();
    let plan = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new().set("crs", projected).set("title", "x"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert!(plan.blocked.is_none(), "{:?}", plan.blocked);
    let note = plan
        .says
        .iter()
        .find(|n| n.key == "frame-projected")
        .unwrap_or_else(|| panic!("{:?}", plan.says));
    assert!(
        note.detail.contains("EPSG:2154") && note.detail.contains("§3.1"),
        "{}",
        note.detail
    );
    // The compound of the same shapes says nothing of its frame.
    let compound = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new().set("title", "x"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert!(
        compound.says.iter().all(|n| !n.key.starts_with("frame-")),
        "{:?}",
        compound.says
    );
}

/// **Degrees keep their `Geographic2dCrs`, and the plan says it is the named
/// exception** - decision (a) of 24/09: EML 2.3 has no compound over degrees
/// (`LocalEngineering2dCrs.OriginProjectedCrs` is a `ProjectedCrs`, CRS.xsd
/// l. 504).
#[test]
fn a_frame_created_in_degrees_is_the_named_exception() {
    let collection = geojson::read(&lines_geojson()).unwrap();
    let pieces = Pieces::empty().with(
        "lines.geojson",
        Source::Features {
            name: "lines.geojson".into(),
            features: collection.features,
        },
    );
    let plan = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new().set("title", "x"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let roles: Vec<(&str, &str)> = plan
        .objects
        .iter()
        .map(|o| (o.role.as_str(), o.qualified_type.as_str()))
        .collect();
    assert_eq!(
        roles,
        [
            ("crs", "eml23.Geographic2dCrs"),
            ("vertical-crs", "eml23.VerticalCrs"),
            ("representation", "resqml22.PointSetRepresentation")
        ]
    );
    assert!(
        plan.says.iter().any(|n| n.key == "frame-degrees"),
        "{:?}",
        plan.says
    );
}

/// **This test said the opposite until 11/09**, and the change is the
/// correction of a reported defect: *“I took a surface and
/// asked for a boundary line, and the boundary line was not made at
/// all”*, on Volve, which is in RESQML 2.0.1.
///
/// The operation was not at fault: on a 2.2 package it returns five boundary
/// rings and 1,061 points. It refused to **compute** because it
/// could not **write** - and looked up rather than assumed, the CRS reaches
/// only one place of the code, the `LocalCrs` of the geometry written. The
/// rings have no need of it.
///
/// So the plan succeeds, carries its geometry, and **says what is missing**.
#[test]
fn a_crs_of_another_release_only_blocks_the_writing() {
    let pieces = known(Geometry {
        kind: "x",
        patches: vec![patch(0, square(), Topology::Points)],
    });
    let old = Ref::new(
        "cccccccc-0000-4000-8000-000000000003",
        "resqml20.obj_LocalDepth3dCrs",
        "Ancien",
    )
    .dor();
    let plan = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new().set("crs", old),
            &Identity::derived(),
            &when(),
        )
        .expect("the computation succeeds: it does not depend on the CRS");

    // The geometry is there, and that is the whole point.
    assert!(!plan.objects.is_empty(), "the plan carries what it computed");
    assert!(!plan.datasets.is_empty(), "and the buffers that go with it");

    // And what is missing is named, at both ends: the system and the release.
    let why = plan.blocked.as_ref().expect("the writing is blocked").to_string();
    assert!(why.contains("resqml20.obj_LocalDepth3dCrs"), "{why}");
    assert!(why.contains("2.2"), "{why}");
}

/// And the block travels to the writing, which is the only place that refuses.
///
/// Without this half, the blocked plan would simply be written: a package half
/// 2.0.1 half 2.2, exactly what the original refusal protected against.
#[test]
fn a_blocked_plan_writes_nothing_and_says_why() {
    let pieces = known(Geometry {
        kind: "x",
        patches: vec![patch(0, square(), Topology::Points)],
    });
    let old = Ref::new(
        "cccccccc-0000-4000-8000-000000000003",
        "resqml20.obj_LocalDepth3dCrs",
        "Ancien",
    )
    .dor();
    let plan = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new().set("crs", old),
            &Identity::derived(),
            &when(),
        )
        .unwrap();

    let schemas = std::sync::Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "bloque.epc", schemas).expect("a new package");
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.written.is_empty(), "nothing is written");
    let said = made.refused.join(" ");
    assert!(said.contains("resqml20.obj_LocalDepth3dCrs"), "{said}");
    // And the refusal does **not** say that the plan is empty: it is not, and
    // saying so would send one looking for the wrong thing.
    assert!(!said.contains("holds no object"), "{said}");
}

// -- polylines --------------------------------------------------------------

#[test]
fn polylines_said_closed_by_the_shape_are_written_closed_without_a_note() {
    let geometry = Geometry {
        kind: "contours",
        patches: vec![patch(
            0,
            [square(), square()].concat(),
            Topology::Polylines(vec![4, 4]),
        )],
    };
    let mut pieces = known(Geometry {
        kind: "x",
        patches: vec![],
    });
    pieces = pieces.with(
        "contours",
        Source::Shape {
            geometry,
            closed: Some(vec![true, false]),
        },
    );
    let plan = Registry::new()
        .plan(
            "polylines",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let patch = &plan.objects[0].json["LinePatch"][0];
    assert_eq!(patch["NodeCount"], json!(8));
    // A closed square has 4 intervals, an open one 3.
    assert_eq!(patch["IntervalCount"], json!(7));
    assert_eq!(patch["ClosedPolylines"]["$type"], json!("eml23.BooleanXmlArray"));
    assert_eq!(patch["ClosedPolylines"]["Values"], json!([true, false]));
    assert!(plan.says.is_empty(), "{:?}", plan.says);
}

#[test]
fn a_run_whose_ends_coincide_is_seen_closed_the_repeat_dropped_and_the_plan_says_so() {
    // To break: the repeat kept (RESQML does not repeat the first node), or the
    // closure taken without a note - a ring seen where a line was meant.
    let mut ring = square();
    ring.push(ring[0]);
    let geometry = Geometry {
        kind: "x",
        patches: vec![patch(0, ring, Topology::Polylines(vec![5]))],
    };
    let pieces = known(geometry);
    let plan = Registry::new()
        .plan(
            "polylines",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let patch = &plan.objects[0].json["LinePatch"][0];
    assert_eq!(patch["NodeCount"], json!(4));
    assert_eq!(
        patch["ClosedPolylines"],
        json!({ "$type": "eml23.BooleanConstantArray", "Value": true, "Count": 1 })
    );
    assert_eq!(
        plan.says.iter().map(|n| n.key).collect::<Vec<_>>(),
        vec!["closed-seen"]
    );

    // Forced open: the repeat stays, because it is then a real node.
    let plan = Registry::new()
        .plan(
            "polylines",
            &pieces,
            &Choices::new().set("closed", false),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(plan.objects[0].json["LinePatch"][0]["NodeCount"], json!(5));
    assert!(plan.says.is_empty());
}

#[test]
fn a_feature_collections_areas_are_rings_and_its_lines_are_open() {
    let collection = geojson::read(&lines_geojson()).unwrap();
    let pieces = Pieces::empty().with(
        "lines.geojson",
        Source::Features {
            name: "lines.geojson".into(),
            features: collection.features,
        },
    );
    let plan = Registry::new()
        .plan(
            "polylines",
            &pieces,
            &Choices::new().set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    // By role: the frame created over metres is four objects since 24/09 (the
    // compound and its plane joined the two halves), so a rank would name one.
    let patch = &plan.of_role("representation").next().expect("the polylines").json["LinePatch"][0];
    assert_eq!(
        patch["NodeCountPerPolyline"]["Values"],
        json!([3, 4]),
        "the ring's repeat is dropped"
    );
    assert_eq!(patch["ClosedPolylines"]["Values"], json!([false, true]));
    assert!(plan.says.is_empty(), "said by the geometry kind, not seen");
}

#[test]
fn a_shape_without_polylines_is_refused_naming_its_topology() {
    let pieces = known(Geometry {
        kind: "x",
        patches: vec![patch(0, square(), Topology::Points)],
    });
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "polylines")
        .unwrap();
    match offer.fitness {
        Fitness::Unfit(why) => assert!(why.message.contains("no polyline"), "{why}"),
        Fitness::Fit => panic!("points are not polylines"),
    }
}

// -- triangles --------------------------------------------------------------

#[test]
fn two_patches_make_two_triangle_patches_with_local_indices() {
    let tri = Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]);
    let geometry = Geometry {
        kind: "surface",
        patches: vec![patch(0, square(), tri.clone()), patch(1, square(), tri)],
    };
    let pieces = known(geometry);
    let plan = Registry::new()
        .plan(
            "triangles",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let object = &plan.objects[0];
    assert_eq!(object.qualified_type, "resqml22.TriangulatedSetRepresentation");
    assert_eq!(object.json["TrianglePatch"].as_array().unwrap().len(), 2);
    assert_eq!(object.json["TrianglePatch"][1]["NodeCount"], json!(4));
    assert_eq!(plan.datasets.len(), 4);
    assert_eq!(plan.datasets[1].dimensions, vec![2, 3]);
    match &plan.datasets[1].values {
        energyml_build::operation::Numbers::I64(v) => assert_eq!(v, &vec![0, 1, 2, 0, 2, 3]),
        _ => panic!("integers"),
    }
}

#[test]
fn a_triangle_naming_a_node_the_patch_lacks_is_refused() {
    let geometry = Geometry {
        kind: "surface",
        patches: vec![patch(0, square(), Topology::Triangles(vec![[0, 1, 9]]))],
    };
    let why = Registry::new()
        .plan(
            "triangles",
            &known(geometry),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("out of range");
    assert!(
        why.message().contains("node 9") && why.message().contains("holds 4"),
        "{why}"
    );
}

#[test]
fn quads_are_refused_rather_than_triangulated() {
    let geometry = Geometry {
        kind: "grid",
        patches: vec![patch(0, square(), Topology::Quads(vec![[0, 1, 2, 3]]))],
    };
    let registry = Registry::new();
    let pieces = known(geometry);
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "triangles")
        .unwrap();
    match offer.fitness {
        Fitness::Unfit(why) => assert!(why.message.contains("quadrilaterals"), "{why}"),
        Fitness::Fit => panic!("quads need a triangulation nobody asked for"),
    }
}

// -- property and colormap --------------------------------------------------

fn surface_with_field(values: Vec<f64>) -> Pieces {
    Pieces::empty()
        .with(
            "horizon",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml22.TriangulatedSetRepresentation",
                    "Top",
                )),
                crs: Some(crs_ref()),
                node_count: Some(4),
                ..Default::default()
            }),
        )
        .with(
            "elevation",
            Source::Field {
                over: "horizon".into(),
                on: "nodes".into(),
                values,
                uom: Some("m".into()),
            },
        )
}

#[test]
fn a_field_over_a_representation_becomes_a_property_with_its_count_checked() {
    // To break: the representation not suggested from the piece the field
    // names, or a field of the wrong length accepted - values on the wrong
    // elements is the silent failure this check exists for.
    let pieces = surface_with_field(vec![-100.0, -110.0, -120.0, -130.0]);
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "property")
        .unwrap();
    assert_eq!(offer.fitness, Fitness::Fit);
    let representation = offer.needs.iter().find(|s| s.name == "representation").unwrap();
    assert_eq!(
        representation.suggested.as_ref().map(|v| v["Uuid"].clone()),
        Some(json!("bbbbbbbb-0000-4000-8000-000000000002"))
    );
    assert_eq!(
        offer.needs.iter().find(|s| s.name == "uom").unwrap().suggested,
        Some(json!("m"))
    );

    let kind = Ref::new(
        "dddddddd-0000-4000-8000-000000000009",
        "eml23.PropertyKind",
        "elevation",
    )
    .dor();
    let plan = registry
        .plan(
            "property",
            &pieces,
            &Choices::new().set("propertyKind", kind),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let object = &plan.objects[0];
    assert_eq!(object.qualified_type, "resqml22.ContinuousProperty");
    assert_eq!(object.json["IndexableElement"], json!("nodes"));
    assert_eq!(object.json["Uom"], json!("m"));
    assert_eq!(
        object.json["SupportingRepresentation"]["Uuid"],
        json!("bbbbbbbb-0000-4000-8000-000000000002")
    );
    assert_eq!(plan.datasets[0].dimensions, vec![4]);
    assert!(plan.says.is_empty(), "{:?}", plan.says);

    let kind = Ref::new(
        "dddddddd-0000-4000-8000-000000000009",
        "eml23.PropertyKind",
        "elevation",
    )
    .dor();
    let why = registry
        .plan(
            "property",
            &surface_with_field(vec![1.0, 2.0, 3.0]),
            &Choices::new().set("propertyKind", kind),
            &Identity::derived(),
            &when(),
        )
        .expect_err("three values for four nodes");
    assert!(
        why.message().contains("4 node(s)") && why.message().contains("3 value(s)"),
        "{why}"
    );
}

#[test]
fn a_property_kind_is_required_and_an_unknown_indexable_element_is_refused() {
    let pieces = surface_with_field(vec![1.0, 2.0, 3.0, 4.0]);
    let why = Registry::new()
        .plan(
            "property",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no kind");
    assert!(why.message().contains("`propertyKind`"), "{why}");
    let odd = Pieces::empty().with(
        "f",
        Source::Field {
            over: "x".into(),
            on: "corners".into(),
            values: vec![1.0],
            uom: None,
        },
    );
    let registry = Registry::new();
    match registry
        .offers(&odd)
        .into_iter()
        .find(|o| o.operation.name() == "property")
        .unwrap()
        .fitness
    {
        Fitness::Unfit(why) => assert!(why.message.contains("corners"), "{why}"),
        Fitness::Fit => panic!("corners are not an indexable element"),
    }
}

#[test]
fn a_ramp_becomes_a_colour_map_of_flat_bands() {
    let hsv = |hue: f64| energyml_domain::color::Hsv {
        hue,
        saturation: 1.0,
        value: 1.0,
        alpha: 1.0,
        title: None,
    };
    let pieces = Pieces::empty().with(
        "bandes",
        Source::Ramp {
            levels: vec![0.0, 10.0, 20.0],
            colours: vec![hsv(0.0), hsv(60.0), hsv(120.0)],
        },
    );
    let plan = Registry::new()
        .plan(
            "colormap",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let object = &plan.objects[0];
    assert_eq!(object.qualified_type, "resqml22.ContinuousColorMap");
    assert_eq!(object.title, "bandes");
    // Flat bands: each band's colour is stated at both its ends, and the last
    // band, open-ended, at its start alone - 2n − 1 stops for n levels.
    assert_eq!(object.json["Entry"].as_array().unwrap().len(), 5);
    assert!(plan.datasets.is_empty());
    let odd = Pieces::empty().with(
        "r",
        Source::Ramp {
            levels: vec![0.0],
            colours: vec![hsv(0.0)],
        },
    );
    let registry = Registry::new();
    match registry
        .offers(&odd)
        .into_iter()
        .find(|o| o.operation.name() == "colormap")
        .unwrap()
        .fitness
    {
        Fitness::Unfit(why) => assert!(why.message.contains("two levels"), "{why}"),
        Fitness::Fit => panic!("one level is no band"),
    }
}

// -- boundary ---------------------------------------------------------------

#[test]
fn a_boundary_is_a_feature_an_interpretation_and_a_ring_linked_before_they_exist() {
    // What the old promoter alone could do. To break: the representation not
    // pointing at the interpretation, the interpretation not at the feature,
    // or the ring written open.
    let geometry = Geometry {
        kind: "ring",
        patches: vec![patch(0, square(), Topology::Polylines(vec![4]))],
    };
    let plan = Registry::new()
        .plan(
            "boundary",
            &known(geometry),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    assert_eq!(roles, vec!["feature", "interpretation", "representation"]);
    let feature = &plan.objects[0];
    let interpretation = &plan.objects[1];
    let representation = &plan.objects[2];
    assert_eq!(feature.json["CulturalFeatureKind"], json!("model frontier"));
    assert_eq!(
        interpretation.json["InterpretedFeature"]["Uuid"],
        json!(feature.uuid)
    );
    assert_eq!(
        representation.json["RepresentedObject"]["Uuid"],
        json!(interpretation.uuid)
    );
    assert_eq!(
        representation.json["LinePatch"][0]["ClosedPolylines"]["Value"],
        json!(true)
    );
    assert!(plan.says.is_empty(), "said closed, nothing detected");
    assert_eq!(plan.activity.recipe, "boundary");

    let pipeline = Registry::new()
        .plan(
            "boundary",
            &known(Geometry {
                kind: "line",
                patches: vec![patch(0, square(), Topology::Polylines(vec![4]))],
            }),
            &Choices::new().set("featureKind", "pipeline").set("closed", false),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(pipeline.objects[0].json["CulturalFeatureKind"], json!("pipeline"));
    assert_eq!(
        pipeline.objects[2].json["LinePatch"][0]["ClosedPolylines"]["Value"],
        json!(false)
    );
}

// -- outline ----------------------------------------------------------------

/// A 3 × 3 grid of nodes, eight triangles: a square with a node mid-side.
fn grid_surface() -> Geometry {
    let points: Vec<[f64; 3]> = (0..3)
        .flat_map(|j| (0..3).map(move |i| [i as f64 * 10.0, j as f64 * 10.0, -100.0]))
        .collect();
    let mut triangles = Vec::new();
    for j in 0..2u32 {
        for i in 0..2u32 {
            let a = j * 3 + i;
            triangles.push([a, a + 1, a + 4]);
            triangles.push([a, a + 4, a + 3]);
        }
    }
    Geometry {
        kind: "surface",
        patches: vec![patch(0, points, Topology::Triangles(triangles))],
    }
}

#[test]
fn the_outline_of_a_surface_is_its_boundary_simplified_and_written_closed() {
    // To break: an interior edge on the rim, a mid-side node kept, the ring
    // written open, or the plan silent on what it did.
    let pieces = known(grid_surface());
    let plan = Registry::new()
        .plan("outline", &pieces, &Choices::new(), &Identity::derived(), &when())
        .unwrap();
    let object = &plan.objects[0];
    assert_eq!(object.qualified_type, "resqml22.PolylineSetRepresentation");
    let patch = &object.json["LinePatch"][0];
    assert_eq!(
        patch["NodeCountPerPolyline"]["Values"],
        json!([4]),
        "four corners, the mid-side nodes dropped"
    );
    assert_eq!(
        patch["ClosedPolylines"],
        json!({ "$type": "eml23.BooleanConstantArray", "Value": true, "Count": 1 })
    );
    assert_eq!(patch["IntervalCount"], json!(4));
    assert_eq!(
        plan.says.iter().map(|n| n.key).collect::<Vec<_>>(),
        vec!["boundary"]
    );
    assert_eq!(plan.activity.recipe, "outline");
    let how = plan.activity.parameters.iter().find_map(|p| match p {
        energyml_domain::provenance::Parameter::Text { title, value } if title == "how" => {
            Some(value.clone())
        }
        _ => None,
    });
    assert_eq!(how.as_deref(), Some("boundary"));
}

#[test]
fn the_outline_of_a_cloud_is_its_hull_and_says_it_is_not_a_boundary() {
    let mut cloud = square();
    cloud.push([5.0, 5.0, 0.0]);
    cloud.push([2.0, 7.0, 0.0]);
    let pieces = known(Geometry {
        kind: "cloud",
        patches: vec![patch(0, cloud, Topology::Points)],
    });
    let plan = Registry::new()
        .plan("outline", &pieces, &Choices::new(), &Identity::derived(), &when())
        .unwrap();
    assert_eq!(
        plan.objects[0].json["LinePatch"][0]["NodeCountPerPolyline"]["Values"],
        json!([4])
    );
    assert_eq!(plan.says[0].key, "hull");
    assert!(
        plan.says[0].detail.contains("not its boundary"),
        "{}",
        plan.says[0].detail
    );
}

#[test]
fn a_closed_surface_has_nothing_to_outline() {
    let tetra = Geometry {
        kind: "closed",
        patches: vec![patch(
            0,
            vec![
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [0.0, 10.0, 0.0],
                [0.0, 0.0, 10.0],
            ],
            Topology::Triangles(vec![[0, 1, 2], [0, 3, 1], [1, 3, 2], [2, 3, 0]]),
        )],
    };
    let why = Registry::new()
        .plan(
            "outline",
            &known(tetra),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no boundary");
    assert!(why.message().contains("closed"), "{why}");
}

#[test]
fn a_bow_tie_is_refused_naming_the_node() {
    let bow = Geometry {
        kind: "bow",
        patches: vec![patch(
            0,
            vec![
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [10.0, 10.0, 0.0],
                [-10.0, 0.0, 0.0],
                [-10.0, -10.0, 0.0],
            ],
            Topology::Triangles(vec![[0, 1, 2], [0, 3, 4]]),
        )],
    };
    let why = Registry::new()
        .plan(
            "outline",
            &known(bow),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("not simple");
    assert!(why.message().contains("node 0"), "{why}");
}

// -- the whole way, when the arrays can be written ---------------------------

// -- grid -------------------------------------------------------------------

fn lattice(z: Vec<f64>) -> Pieces {
    Pieces::empty()
        .with(
            "map",
            Source::Lattice(Lattice {
                origin: [450000.0, 6780000.0],
                spacing: [50.0, 25.0],
                counts: [3, 4],
                z,
            }),
        )
        .with(
            "frame",
            Source::Object(Known {
                crs: Some(crs_ref()),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        )
}

#[test]
fn a_lattice_becomes_a_map_with_the_slow_axis_first() {
    // To break: the dimensions swapped (a 3 × 4 read as a 4 × 3 shifts every
    // row), a spacing count equal to the node count, or the z array written
    // in another order than the lattice declares.
    let z: Vec<f64> = (0..12).map(|n| n as f64).collect();
    let pieces = lattice(z.clone());
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "grid")
        .unwrap();
    assert_eq!(offer.fitness, Fitness::Fit);
    let plan = registry
        .plan(
            "grid",
            &pieces,
            &Choices::new().set("title", "socle"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(plan.objects.len(), 1);
    let object = &plan.objects[0];
    assert_eq!(object.qualified_type, "resqml22.Grid2dRepresentation");
    assert_eq!(object.json["FastestAxisCount"], json!(3));
    assert_eq!(object.json["SlowestAxisCount"], json!(4));
    let support = &object.json["Geometry"]["Points"]["SupportingGeometry"];
    assert_eq!(support["Origin"]["Coordinate1"], json!(450000.0));
    let dimensions = support["Dimension"].as_array().unwrap();
    assert_eq!(
        dimensions[0]["Spacing"]["Count"],
        json!(3),
        "the slow axis first: 3 spacings of 25 along y"
    );
    assert_eq!(dimensions[0]["Spacing"]["Value"], json!(25.0));
    assert_eq!(dimensions[1]["Spacing"]["Count"], json!(2));
    assert_eq!(dimensions[1]["Spacing"]["Value"], json!(50.0));
    assert_eq!(plan.datasets.len(), 1);
    assert_eq!(plan.datasets[0].dimensions, vec![4, 3]);
    assert_eq!(
        object.json["Geometry"]["Points"]["ZValues"]["Values"]["ExternalDataArrayPart"][0]["Count"],
        json!([4, 3])
    );
    assert!(plan.says.is_empty());
    assert!(plan.activity.parameters.iter().any(|p| matches!(p, energyml_domain::provenance::Parameter::Double { title, value, .. } if title == "nodeCount" && *value == 12.0)));

    // A hole is written as NaN and said, never filled.
    let mut holed = z.clone();
    holed[5] = f64::NAN;
    let plan = registry
        .plan(
            "grid",
            &lattice(holed),
            &Choices::new().set("title", "socle"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(plan.says.len(), 1);
    assert_eq!(plan.says[0].key, "holes");
    assert!(plan.says[0].detail.contains("1 of 12"), "{}", plan.says[0].detail);
}

#[test]
fn a_lattice_is_refused_when_its_counts_and_heights_disagree_or_its_frame_would_bend_it() {
    let registry = Registry::new();
    let title = || Choices::new().set("title", "socle");
    // Eleven heights for twelve nodes: both numbers in the refusal.
    let why = registry
        .plan(
            "grid",
            &lattice(vec![0.0; 11]),
            &title(),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(
        why.message().contains("12") && why.message().contains("11"),
        "{}",
        why.message()
    );
    // A single row is not a grid.
    let flat = Pieces::empty()
        .with(
            "map",
            Source::Lattice(Lattice {
                origin: [0.0, 0.0],
                spacing: [1.0, 1.0],
                counts: [1, 4],
                z: vec![0.0; 4],
            }),
        )
        .with(
            "frame",
            Source::Object(Known {
                crs: Some(crs_ref()),
                ..Default::default()
            }),
        );
    let why = registry
        .plan("grid", &flat, &title(), &Identity::derived(), &when())
        .unwrap_err();
    assert!(why.message().contains("at least 2"), "{}", why.message());
    // A frame created here that would place the lattice is refused by name;
    // the same system on both sides is taken.
    let bare = Pieces::empty().with(
        "map",
        Source::Lattice(Lattice {
            origin: [0.0, 0.0],
            spacing: [1.0, 1.0],
            counts: [2, 2],
            z: vec![0.0; 4],
        }),
    );
    let why = registry
        .plan(
            "grid",
            &bare,
            &title().set("sourceEpsg", 4326).set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(
        why.message().contains("4326") && why.message().contains("2154") && why.message().contains("bend"),
        "{}",
        why.message()
    );
    let plan = registry
        .plan(
            "grid",
            &bare,
            &title().set("sourceEpsg", 2154).set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    // The frame created over metres is the compound and its three parts since
    // 24/09 - `crs` the compound the lattice names (RESQML TRG 2.2 §3.1 p. 22).
    assert_eq!(
        plan.objects.iter().map(|o| o.role.as_str()).collect::<Vec<_>>(),
        vec![
            "projected-crs",
            "vertical-crs",
            "local-2d-crs",
            "crs",
            "representation"
        ]
    );
    let frame = plan.of_role("crs").next().unwrap();
    assert_eq!(
        plan.of_role("representation").next().unwrap().json["Geometry"]["LocalCrs"]["Uuid"],
        json!(frame.uuid)
    );
    // Two lattices are not one grid.
    let two = lattice(vec![0.0; 12]).with(
        "other",
        Source::Lattice(Lattice {
            origin: [0.0, 0.0],
            spacing: [1.0, 1.0],
            counts: [2, 2],
            z: vec![0.0; 4],
        }),
    );
    let unfit = registry
        .offers(&two)
        .into_iter()
        .find(|o| o.operation.name() == "grid")
        .unwrap()
        .fitness;
    assert!(
        matches!(unfit, Fitness::Unfit(ref why) if why.message.contains("one grid")),
        "{unfit:?}"
    );
}

// -- interpolate ------------------------------------------------------------

/// Samples on a plane `z = 100 + 2x + 3y`, as a point cloud in a frame of the
/// package: an interpolator that reproduces a plane is the analytic case.
fn samples_on_a_plane() -> Pieces {
    let points: Vec<[f64; 3]> = (0..4)
        .flat_map(|j| {
            (0..4).map(move |i| {
                let (x, y) = (i as f64 * 30.0, j as f64 * 30.0);
                [x, y, 100.0 + 2.0 * x + 3.0 * y]
            })
        })
        .collect();
    known(Geometry {
        kind: "cloud",
        patches: vec![patch(0, points, Topology::Points)],
    })
}

#[test]
fn an_interpolation_writes_a_grid_of_the_samples_extent_in_lattice_order() {
    // To break: the queries built in the other order (the map would be
    // transposed), the extent taken from something other than the samples, or
    // the lattice placed rather than written where it was computed.
    let pieces = samples_on_a_plane();
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "interpolate")
        .unwrap();
    assert_eq!(offer.fitness, Fitness::Fit);

    let choices = Choices::new()
        .set("method", "rbf")
        .set("kernel", "thin-plate")
        .set("spacing", 30.0)
        .set("title", "socle");
    let plan = registry
        .plan("interpolate", &pieces, &choices, &Identity::derived(), &when())
        .unwrap();
    let object = &plan.objects[0];
    assert_eq!(object.qualified_type, "resqml22.Grid2dRepresentation");
    // Four samples each way at 30 apart is a 4 by 4 grid on the same nodes.
    assert_eq!(object.json["FastestAxisCount"], json!(4));
    assert_eq!(object.json["SlowestAxisCount"], json!(4));
    let lattice = &object.json["Geometry"]["Points"]["SupportingGeometry"];
    assert_eq!(lattice["Origin"]["Coordinate1"], json!(0.0));
    assert_eq!(lattice["Origin"]["Coordinate2"], json!(0.0));
    assert_eq!(plan.datasets[0].dimensions, vec![4, 4]);

    // A radial basis reproduces a plane, so every node is on it - and node
    // `j * ni + i` is at `(i, j)`, which is what a transposition would break.
    let z = values_of(&plan);
    assert_eq!(z.len(), 16);
    for j in 0..4 {
        for i in 0..4 {
            let (x, y) = (i as f64 * 30.0, j as f64 * 30.0);
            let at = z[j * 4 + i];
            assert!(
                (at - (100.0 + 2.0 * x + 3.0 * y)).abs() < 1e-6,
                "node ({i}, {j}) is {at}"
            );
        }
    }
    assert!(
        plan.says
            .iter()
            .any(|n| n.key == "samples" && n.detail.contains("16 sample(s)")),
        "{:?}",
        plan.says
    );
    assert!(plan.activity.parameters.iter().any(
        |p| matches!(p, energyml_domain::provenance::Parameter::Text { title, value } if title == "method" && value == "rbf")
    ));
}

#[test]
fn a_margin_widens_the_grid_and_the_origin_it_wrote_is_reported() {
    // The extent is not a number anybody stated, so the activity carries it.
    let pieces = samples_on_a_plane();
    let choices = Choices::new()
        .set("method", "idw")
        .set("spacing", 30.0)
        .set("margin", 30.0)
        .set("title", "socle");
    let plan = Registry::new()
        .plan("interpolate", &pieces, &choices, &Identity::derived(), &when())
        .unwrap();
    assert_eq!(
        plan.objects[0].json["FastestAxisCount"],
        json!(6),
        "one cell of margin on each side"
    );
    let lattice = &plan.objects[0].json["Geometry"]["Points"]["SupportingGeometry"];
    assert_eq!(lattice["Origin"]["Coordinate1"], json!(-30.0));
    let reported = |name: &str| {
        plan.activity.parameters.iter().find_map(|p| match p {
            energyml_domain::provenance::Parameter::Double { title, value, .. } if title == name => {
                Some(*value)
            }
            _ => None,
        })
    };
    assert_eq!(reported("originX"), Some(-30.0));
    assert_eq!(reported("originY"), Some(-30.0));
    assert_eq!(reported("sampleCount"), Some(16.0));
}

#[test]
fn a_field_over_the_samples_is_what_is_mapped_rather_than_their_elevation() {
    // The same nodes, valued twice: *interpolate this property* and
    // *interpolate this elevation* are one computation over different numbers.
    let base = samples_on_a_plane();
    let with_field = base.clone().with(
        "phi",
        Source::Field {
            over: "shape".into(),
            on: "nodes".into(),
            values: vec![0.25; 16],
            uom: Some("Euc".into()),
        },
    );
    let choices = Choices::new()
        .set("method", "idw")
        .set("spacing", 30.0)
        .set("title", "socle");
    let plan = Registry::new()
        .plan(
            "interpolate",
            &with_field,
            &choices,
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    for z in values_of(&plan) {
        assert!(
            (z - 0.25).abs() < 1e-9,
            "the field's value, not the elevation: {z}"
        );
    }
    assert!(
        plan.says.iter().any(|n| n.detail.contains("valued by the field")),
        "{:?}",
        plan.says
    );
}

#[test]
fn a_node_no_sample_reaches_is_a_hole_and_both_ends_count_it() {
    // IDW with a radius leaves nodes empty rather than extrapolating; the grid
    // writes them as NaN and says how many, and so does this.
    let pieces = samples_on_a_plane();
    let choices = Choices::new()
        .set("method", "idw")
        .set("spacing", 30.0)
        .set("margin", 60.0)
        .set("radius", 10.0)
        .set("title", "socle");
    let plan = Registry::new()
        .plan("interpolate", &pieces, &choices, &Identity::derived(), &when())
        .unwrap();
    let z = values_of(&plan);
    let holes = z.iter().filter(|v| v.is_nan()).count();
    assert!(
        holes > 0,
        "a 60 wide margin at a radius of 10 leaves corners empty"
    );
    assert!(
        plan.says
            .iter()
            .any(|n| n.key == "unreached" && n.detail.contains(&holes.to_string())),
        "{:?}",
        plan.says
    );
    assert!(
        plan.says.iter().any(|n| n.key == "holes"),
        "the grid counts them too: {:?}",
        plan.says
    );
}

#[test]
fn kriging_without_a_model_fits_one_and_says_so() {
    let pieces = samples_on_a_plane();
    let choices = Choices::new()
        .set("method", "kriging")
        .set("spacing", 30.0)
        .set("title", "socle");
    let plan = Registry::new()
        .plan("interpolate", &pieces, &choices, &Identity::derived(), &when())
        .unwrap();
    let note = plan
        .says
        .iter()
        .find(|n| n.key == "fitted")
        .expect("a model nobody stated is a decision");
    assert!(
        note.detail.contains("nugget") && note.detail.contains("range"),
        "{}",
        note.detail
    );
    // A stated model without its three numbers is refused by name.
    let short = Choices::new()
        .set("method", "kriging")
        .set("model", "spherical")
        .set("spacing", 30.0)
        .set("title", "socle");
    let why = Registry::new()
        .plan("interpolate", &pieces, &short, &Identity::derived(), &when())
        .unwrap_err();
    assert!(why.message().contains("nugget"), "{}", why.message());
}

#[test]
fn an_interpolation_refuses_a_spacing_that_makes_no_grid_or_too_much_of_one() {
    let pieces = samples_on_a_plane();
    let registry = Registry::new();
    let with = |spacing: f64| {
        Choices::new()
            .set("method", "idw")
            .set("spacing", spacing)
            .set("title", "socle")
    };
    // Wider than the samples: one node on an axis is not a grid.
    let why = registry
        .plan(
            "interpolate",
            &pieces,
            &with(1000.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(why.message().contains("at least 2"), "{}", why.message());
    // A typo in the other direction is refused with the count, not attempted.
    let why = registry
        .plan(
            "interpolate",
            &pieces,
            &with(0.0001),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(why.message().contains("widen the spacing"), "{}", why.message());
    // And a bag with nothing to interpolate says what it holds.
    let unfit = registry
        .offers(&Pieces::empty())
        .into_iter()
        .find(|o| o.operation.name() == "interpolate")
        .unwrap()
        .fitness;
    assert!(
        matches!(unfit, Fitness::Unfit(ref why) if why.message.contains("samples")),
        "{unfit:?}"
    );
}

// -- surface-map ------------------------------------------------------------

/// A plane falling toward +x, in the usual RESQML convention where the third
/// coordinate is a **depth**: nine nodes on a 3 x 3 grid, the depth growing
/// with x so the surface falls that way.
fn dipping_plane(slope_degrees: f64) -> Geometry {
    let rise = slope_degrees.to_radians().tan();
    let points: Vec<[f64; 3]> = (0..3)
        .flat_map(|j| (0..3).map(move |i| [i as f64 * 10.0, j as f64 * 10.0, i as f64 * 10.0 * rise]))
        .collect();
    let mut triangles = Vec::new();
    for j in 0..2u32 {
        for i in 0..2u32 {
            let a = j * 3 + i;
            triangles.push([a, a + 1, a + 4]);
            triangles.push([a, a + 4, a + 3]);
        }
    }
    Geometry {
        kind: "surface",
        patches: vec![patch(0, points, Topology::Triangles(triangles))],
    }
}

/// **A 2D grid is measured as a lattice** - Lutetia's DTM (25/09). The same
/// plane as [`dipping_plane`], its nodes a 3 x 3 lattice at map coordinates
/// with no listed face: fit, 30 degrees of dip and 90 of azimuth everywhere,
/// read by central differences on the positions; a curvature is refused by
/// name, never computed on cells cut in two.
#[test]
fn a_grid_is_measured_as_a_lattice_and_its_curvature_is_refused_by_name() {
    let rise = 30f64.to_radians().tan();
    let points: Vec<[f64; 3]> = (0..3)
        .flat_map(|j| {
            (0..3).map(move |i| {
                [
                    651_000.0 + i as f64 * 10.0,
                    6_861_000.0 + j as f64 * 10.0,
                    i as f64 * 10.0 * rise,
                ]
            })
        })
        .collect();
    let geometry = Geometry {
        kind: "grid",
        patches: vec![patch(
            0,
            points,
            Topology::Grid {
                fastest: 3,
                slowest: 3,
            },
        )],
    };
    let pieces = surface(geometry, Some(false));
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "surface-map")
        .unwrap();
    assert_eq!(offer.fitness, Fitness::Fit);
    let plan = registry
        .plan(
            "surface-map",
            &pieces,
            &map_choices("dip"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let dip = values_of(&plan);
    assert_eq!(dip.len(), 9);
    assert!(dip.iter().all(|value| (value - 30.0).abs() < 1e-9), "{dip:?}");
    let plan = registry
        .plan(
            "surface-map",
            &pieces,
            &map_choices("azimuth"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let azimuth = values_of(&plan);
    assert!(
        azimuth.iter().all(|value| (value - 90.0).abs() < 1e-9),
        "{azimuth:?}"
    );
    let why = registry
        .plan(
            "surface-map",
            &pieces,
            &map_choices("mean-curvature"),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no curvature on a lattice")
        .to_string();
    assert!(why.contains("2D grid"), "{why}");
}

/// The surface and what the reader knows of it: its reference, its frame, how
/// many nodes it places, and which way its vertical axis points.
fn surface(geometry: Geometry, up: Option<bool>) -> Pieces {
    let nodes = geometry.point_count();
    Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry,
                closed: None,
            },
        )
        .with(
            "object",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml22.TriangulatedSetRepresentation",
                    "The source",
                )),
                crs: Some(crs_ref()),
                vertical_uom: Some("m".into()),
                vertical_up: up,
                node_count: Some(nodes),
                ..Default::default()
            }),
        )
}

fn map_choices(what: &str) -> Choices {
    Choices::new().set("what", what).set("vertical", "depth")
}

/// The value of every node of a plan's one dataset.
fn values_of(plan: &energyml_build::operation::Plan) -> Vec<f64> {
    match &plan.datasets[0].values {
        energyml_build::operation::Numbers::F64(values) => values.clone(),
        other => panic!("not floating point: {other:?}"),
    }
}

#[test]
fn a_dip_map_is_the_angle_the_plane_makes_and_the_azimuth_the_way_it_falls() {
    // The answer is known analytically, which is the point of testing it on a
    // plane. To break: the vertical convention ignored (the azimuth would
    // point the other way), the values written in another order than the
    // nodes, or the kind taken from anywhere but the dictionary.
    let pieces = surface(dipping_plane(30.0), Some(false));
    let registry = Registry::new();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "surface-map")
        .unwrap();
    assert_eq!(offer.fitness, Fitness::Fit);
    // Which way is up is suggested from the CRS the reader read, never guessed.
    assert_eq!(
        offer
            .needs
            .iter()
            .find(|s| s.name == "vertical")
            .unwrap()
            .suggested,
        Some(json!("depth"))
    );

    let plan = registry
        .plan(
            "surface-map",
            &pieces,
            &map_choices("dip"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(plan.objects.len(), 1);
    let object = &plan.objects[0];
    assert_eq!(object.qualified_type, "resqml22.ContinuousProperty");
    assert_eq!(object.title, "The source - dip");
    assert_eq!(object.json["Uom"], json!("dega"));
    assert_eq!(object.json["IndexableElement"], json!("nodes"));
    // The kind is the dictionary's, looked up by title.
    assert_eq!(object.json["PropertyKind"]["Title"], json!("dip angle"));
    assert_eq!(
        object.json["PropertyKind"]["QualifiedType"],
        json!("eml23.PropertyKind")
    );
    assert_eq!(
        object.json["SupportingRepresentation"]["Uuid"],
        json!("bbbbbbbb-0000-4000-8000-000000000002")
    );
    let dip = values_of(&plan);
    assert_eq!(dip.len(), 9);
    for value in &dip {
        assert!(
            (value - 30.0).abs() < 1e-9,
            "{value} is not a 30 degree dip: {dip:?}"
        );
    }
    assert!(plan.says.is_empty(), "{:?}", plan.says);

    // The same surface, its azimuth: it falls toward +x, which is 90 degrees
    // clockwise from north.
    let plan = registry
        .plan(
            "surface-map",
            &pieces,
            &map_choices("azimuth"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(plan.objects[0].title, "The source - dip azimuth");
    assert_eq!(
        plan.objects[0].json["PropertyKind"]["Title"],
        json!("dip azimuth")
    );
    for value in values_of(&plan) {
        assert!((value - 90.0).abs() < 1e-9, "{value} is not due east");
    }

    // Two maps of one surface are two objects, never one written twice.
    let dip = registry
        .plan(
            "surface-map",
            &pieces,
            &map_choices("dip"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let azimuth = registry
        .plan(
            "surface-map",
            &pieces,
            &map_choices("azimuth"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_ne!(dip.objects[0].uuid, azimuth.objects[0].uuid);
    // And derived, so the same run twice writes the same uuid.
    let again = registry
        .plan(
            "surface-map",
            &pieces,
            &map_choices("dip"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(dip.objects[0].uuid, again.objects[0].uuid);
}

#[test]
fn a_flat_surface_has_no_azimuth_and_the_nodes_without_one_are_counted() {
    // A zero would read as due north, which is the one bearing somebody might
    // act on. To break: a NaN turned into a number, or the count dropped.
    let pieces = surface(grid_surface(), Some(false));
    let plan = Registry::new()
        .plan(
            "surface-map",
            &pieces,
            &map_choices("azimuth"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let azimuth = values_of(&plan);
    assert_eq!(azimuth.len(), 9);
    assert!(azimuth.iter().all(|v| v.is_nan()), "{azimuth:?}");
    let note = plan
        .says
        .iter()
        .find(|n| n.key == "undefined")
        .expect("the count is said");
    assert!(note.detail.contains("9 of 9"), "{}", note.detail);
    assert!(plan.activity.parameters.iter().any(
        |p| matches!(p, energyml_domain::provenance::Parameter::Double { title, value, .. } if title == "undefinedCount" && *value == 9.0)
    ));
}

#[test]
fn a_curvature_exists_only_inside_and_a_plane_has_none() {
    // The discrete operators need a closed ring of triangles, which a rim node
    // has not. To break: a boundary node extrapolated, which would draw
    // structure exactly where people look for the edge of the data.
    let pieces = surface(dipping_plane(20.0), Some(false));
    let plan = Registry::new()
        .plan(
            "surface-map",
            &pieces,
            &map_choices("mean-curvature"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert_eq!(plan.objects[0].json["Uom"], json!("1/m"));
    assert_eq!(plan.objects[0].json["PropertyKind"]["Title"], json!("curvature"));
    let mean = values_of(&plan);
    let inside: Vec<f64> = mean.iter().copied().filter(|v| !v.is_nan()).collect();
    assert_eq!(inside.len(), 1, "only the middle node has a ring: {mean:?}");
    assert!(inside[0].abs() < 1e-9, "a plane does not bend: {}", inside[0]);
    assert!(
        plan.says
            .iter()
            .any(|n| n.key == "undefined" && n.detail.contains("8 of 9")),
        "{:?}",
        plan.says
    );
}

#[test]
fn a_surface_map_refuses_what_it_cannot_read_by_name() {
    let registry = Registry::new();
    // No triangle: dip is read off the faces.
    let cloud = surface(
        Geometry {
            kind: "x",
            patches: vec![patch(0, square(), Topology::Points)],
        },
        Some(false),
    );
    let unfit = registry
        .offers(&cloud)
        .into_iter()
        .find(|o| o.operation.name() == "surface-map")
        .unwrap()
        .fitness;
    assert!(
        matches!(unfit, Fitness::Unfit(ref why) if why.message.contains("no triangle")),
        "{unfit:?}"
    );

    let pieces = surface(dipping_plane(10.0), None);
    // Nothing suggests the vertical when the package does not say it.
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "surface-map")
        .unwrap();
    assert_eq!(
        offer
            .needs
            .iter()
            .find(|s| s.name == "vertical")
            .unwrap()
            .suggested,
        None
    );
    // And it is required, so it is refused before any work.
    let why = registry
        .plan(
            "surface-map",
            &pieces,
            &Choices::new().set("what", "dip"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(why.message().contains("vertical"), "{}", why.message());
    // A map nobody writes names the ones that exist.
    let why = registry
        .plan(
            "surface-map",
            &pieces,
            &Choices::new().set("what", "roughness").set("vertical", "depth"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_err();
    assert!(
        why.message().contains("roughness") && why.message().contains("gaussian-curvature"),
        "{}",
        why.message()
    );
}

/// A temporary folder **for this test alone**.
///
/// # The defect it removes, and why it was not visible here
///
/// Two tests of one binary share the process, so two folders
/// named by `process::id()` alone are **the same folder** - and these tests
/// run in parallel and each ends with a `remove_dir_all`. One
/// erases while the other writes.
///
/// On Windows the race is invisible: `remove_dir_all` fails as long as a
/// file is open, and the failure is ignored. On Linux it succeeds, and
/// `save_as` then refuses with `NotFound` - a message that designates neither the faulty
/// test nor the race. CI showed it on 19/09; the suite had passed here
/// for weeks.
///
/// The name of the test makes the collision **impossible by construction**, which is
/// better than a convention: nobody rereads the list of folders before adding
/// one.
///
/// `#[cfg(feature = "hdf5")]` because its three callers are: without
/// that, the configuration without a backend compiles a function that nobody
/// calls, and `lint` runs with `-D warnings`. It is the rule that
/// the CI configuration states for production code - *a dead item in the
/// configuration without a backend is marked* - and it applies to tests.
#[cfg(feature = "hdf5")]
fn scratch(test: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("energyml-build-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the folder of this test");
    dir
}

#[cfg(feature = "hdf5")]
#[test]
fn a_map_is_written_reread_by_the_reader_and_takes_a_property_counted_against_its_nodes() {
    // The round trip the old promoter's tests made in two steps: the grid
    // read back by the real geometry reader with 12 nodes, then a field of
    // 7 values refused naming 12 and 7, and one of 12 planned.
    use std::sync::Arc;
    let dir = scratch("grid-map");
    let out = dir.join("map.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "map.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);
    // The frame the lattice names has to exist for the reader to place it.
    // The compound - role `crs` - and not the first object, which is the
    // `ProjectedCrs` since `crs` writes four (24/09): a lattice named on it would
    // be taken, with a note, and name a frame RESQML TRG 2.2 §3.1 p. 22 does not.
    let frame = Registry::new()
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 2154),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let compound = frame.of_role("crs").next().expect("the frame a geometry names");
    let crs_uuid = compound.uuid.clone();
    let made = energyml_build::apply::apply(&frame, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    let crs = Ref::new(
        crs_uuid.clone(),
        compound.qualified_type.clone(),
        compound.title.clone(),
    );
    let pieces = Pieces::empty()
        .with(
            "map",
            Source::Lattice(Lattice {
                origin: [450000.0, 6780000.0],
                spacing: [50.0, 25.0],
                counts: [3, 4],
                z: (0..12).map(|n| -100.0 - n as f64).collect(),
            }),
        )
        .with(
            "frame",
            Source::Object(Known {
                crs: Some(crs),
                ..Default::default()
            }),
        );
    let plan = Registry::new()
        .plan(
            "grid",
            &pieces,
            &Choices::new().set("title", "socle"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();

    let reread = energyml_storage::EpcStore::open(&out, registry).unwrap();
    let uri = reread
        .find_by_uuid(&plan.objects[0].uuid)
        .expect("the map is in the package")
        .uri
        .clone();
    let read = energyml_build::read::pieces_of_object(&reread, &uri).expect("the reader places the lattice");
    let known = read
        .iter()
        .find_map(|(_, s)| match s {
            Source::Object(k) => Some(k),
            _ => None,
        })
        .unwrap();
    assert_eq!(known.node_count, Some(12));
    let shape = read
        .iter()
        .find_map(|(_, s)| match s {
            Source::Shape { geometry, .. } => Some(geometry),
            _ => None,
        })
        .unwrap();
    let placed: Vec<[f64; 3]> = shape
        .patches
        .iter()
        .flat_map(|p| p.points.iter().copied())
        .collect();
    assert_eq!(placed.len(), 12);
    assert_eq!(
        placed[4],
        [450000.0 + 50.0, 6780000.0 + 25.0, -104.0],
        "node 4 is (i=1, j=1): the fast axis is x"
    );

    let field = |n: usize| {
        let mut pieces = Pieces::empty();
        for (name, source) in read.clone() {
            pieces = pieces.with(name, source);
        }
        let over = read
            .iter()
            .find(|(_, s)| matches!(s, Source::Object(_)))
            .map(|(n, _)| n.clone())
            .unwrap();
        pieces.with(
            "phi",
            Source::Field {
                over,
                on: "nodes".into(),
                values: vec![0.1; n],
                uom: Some("%".into()),
            },
        )
    };
    let kind = json!({ "$type": "eml23.DataObjectReference", "Uuid": "97b2d6a0-0000-4000-8000-000000000001", "QualifiedType": "eml23.PropertyKind", "Title": "porosity" });
    let choices = || {
        Choices::new()
            .set("title", "porosity")
            .set("propertyKind", kind.clone())
    };
    let why = Registry::new()
        .plan("property", &field(7), &choices(), &Identity::derived(), &when())
        .unwrap_err();
    assert!(
        why.message().contains("12") && why.message().contains('7'),
        "{}",
        why.message()
    );
    let plan = Registry::new()
        .plan("property", &field(12), &choices(), &Identity::derived(), &when())
        .unwrap();
    assert_eq!(plan.objects[0].qualified_type, "resqml22.ContinuousProperty");
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(feature = "hdf5")]
#[test]
fn a_point_set_is_written_saved_and_reopened() {
    use std::sync::Arc;
    let pieces = known(Geometry {
        kind: "x",
        patches: vec![patch(0, square(), Topology::Points)],
    });
    let plan = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let dir = scratch("shapes");
    let out = dir.join("points.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "points.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();
    let reread = energyml_storage::EpcStore::open(&out, registry).unwrap();
    assert!(reread.find_by_uuid(&plan.objects[0].uuid).is_some());
    assert!(
        dir.join("data.h5").exists(),
        "the array went to the external file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// -- the computed shape ------------------------------------------------------

/// An operation produces **an artefact first** - and the plan carries it.
///
/// The rule: the predefined operations must
/// first produce an artefact that will then be promoted […] rather than creating
/// the objects directly at the moment of the operation.
///
/// # What would have to break to make it turn red
///
/// Return a plan that writes a representation without carrying the shape that
/// justifies it. The defect would be mute on the package side - the object is right - and
/// total on the scene side: nothing to show before writing, so nothing more to
/// juger.
#[test]
fn a_computed_representation_is_carried_by_the_plan() {
    let registry = Registry::new();
    let cases: [(&str, Pieces); 3] = [
        (
            "pointset",
            known(Geometry {
                kind: "x",
                patches: vec![patch(0, square(), Topology::Points)],
            }),
        ),
        (
            "polylines",
            known(Geometry {
                kind: "x",
                patches: vec![patch(0, square(), Topology::Polylines(vec![4]))],
            }),
        ),
        (
            "triangles",
            known(Geometry {
                kind: "x",
                patches: vec![patch(
                    0,
                    square(),
                    Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
                )],
            }),
        ),
    ];
    for (name, pieces) in cases {
        let plan = registry
            .plan(name, &pieces, &Choices::new(), &Identity::derived(), &when())
            .unwrap();
        let shape = plan
            .shapes
            .first()
            .unwrap_or_else(|| panic!("`{name}` computes a geometry and must carry it"));
        // One root, because none of these three has sub-parts.
        assert_eq!(shape.parent, None, "{name}");
        // And it **names its object**, which links the two halves of the plan
        // without going through the titles, which are not unique.
        let named = shape.object.as_deref().expect("the shape names its object");
        let object = plan
            .objects
            .iter()
            .find(|o| o.uuid == named)
            .unwrap_or_else(|| panic!("`{name}`: {named} is not in the plan"));
        assert_eq!(object.role, "representation", "{name}");
        // The four corners of the square, and nothing else.
        let nodes: usize = shape.geometry.patches.iter().map(|p| p.points.len()).sum();
        assert_eq!(nodes, 4, "{name}");
    }
}

/// **The check that counts.** The shape and the written RESQML are two **independent**
/// statements of the same topology: one says `Topology::Polylines` and its
/// closures, the other `NodeCountPerPolyline` and `ClosedPolylines`. Confronting
/// them is this repository's defence against *two readers agreeing on a
/// total and disagreeing on what it means*.
///
/// One loop too many or too few is not visible on a point cloud: it is
/// exactly the kind of wrong that looks like right.
#[test]
fn the_shape_and_the_resqml_say_the_same_topology() {
    // Two polylines, one closed and one open - otherwise the plan writes a
    // `BooleanConstantArray` and the disagreement would have nowhere to show.
    let ring = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0]];
    let open = [[5.0, 0.0, 0.0], [6.0, 0.0, 0.0]];
    let points: Vec<[f64; 3]> = ring.iter().chain(open.iter()).copied().collect();
    let pieces = Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: Geometry {
                    kind: "x",
                    patches: vec![Patch {
                        index: 0,
                        node_offset: 0,
                        points,
                        topology: Topology::Polylines(vec![3, 2]),
                        closed: vec![true, false],
                    }],
                },
                closed: Some(vec![true, false]),
            },
        )
        .with(
            "object",
            Source::Object(Known {
                reference: Some(Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml22.PolylineSetRepresentation",
                    "The source",
                )),
                crs: Some(crs_ref()),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        );
    let plan = Registry::new()
        .plan(
            "polylines",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();

    let shape = &plan.shapes[0];
    let Topology::Polylines(runs) = &shape.geometry.patches[0].topology else {
        panic!("polylines")
    };
    assert_eq!(runs, &vec![3u32, 2]);
    assert_eq!(shape.geometry.patches[0].closed, vec![true, false]);

    // The other statement, read in the object that the plan will write.
    let object = plan.objects.iter().find(|o| o.role == "representation").unwrap();
    let patch = &object.json["LinePatch"][0];
    let declared: Vec<u32> = patch["NodeCountPerPolyline"]["Values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap() as u32)
        .collect();
    assert_eq!(declared, *runs, "the shape and the RESQML disagree on the runs");
    let closed: Vec<bool> = patch["ClosedPolylines"]["Values"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_bool().unwrap())
        .collect();
    assert_eq!(
        closed, shape.geometry.patches[0].closed,
        "the shape and the RESQML disagree on closure"
    );
    // And the total of the nodes, which is the third statement of the same fact.
    assert_eq!(
        patch["NodeCount"].as_u64().unwrap() as usize,
        shape.geometry.patches[0].points.len()
    );
}

/// The patches of a surface are found one for one, and **each knows where it
/// starts**.
///
/// `node_offset` is the rank of the first node of the patch in the whole shape.
/// The indices of a triangle stay local to the patch - which the operation
/// explicitly refuses otherwise - so without this offset, two patches put end to
/// end would draw the second on the nodes of the first.
#[test]
fn the_patches_of_a_surface_keep_their_rank_and_their_start() {
    let far: Vec<[f64; 3]> = square().into_iter().map(|[x, y, z]| [x + 100.0, y, z]).collect();
    let pieces = known(Geometry {
        kind: "x",
        patches: vec![
            patch(0, square(), Topology::Triangles(vec![[0, 1, 2]])),
            patch(1, far, Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]])),
        ],
    });
    let plan = Registry::new()
        .plan(
            "triangles",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let shape = &plan.shapes[0];
    assert_eq!(shape.geometry.patches.len(), 2);
    assert_eq!(shape.geometry.patches[0].node_offset, 0);
    assert_eq!(shape.geometry.patches[1].node_offset, 4);
    let object = plan.objects.iter().find(|o| o.role == "representation").unwrap();
    assert_eq!(object.json["TrianglePatch"].as_array().unwrap().len(), 2);
}

/// **An absence that is an answer.** Three operations compute no
/// geometry, and three others do not carry one yet - each saying why
/// beside its `Vec::new()`. Confusing them would lose the difference between
/// *there is nothing to carry* and *it has not been done*.
#[test]
fn what_computes_no_geometry_says_so() {
    let plan = Registry::new()
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 2154),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert!(plan.shapes.is_empty(), "a CRS is not a shape");
    assert!(
        !plan.objects.is_empty(),
        "and it does write objects - the two are not the same emptiness"
    );
}

// -- triangulate -------------------------------------------------------------

/// **A point cloud is offered to the triangulation.**
///
/// Reported as a need: selecting a PointSetRepresentation,
/// one could not apply […] the creation of a triangulation
/// (whereas that is exactly what the triangulation does with the button at
/// the top). The module knew how to write triangles it was given, not how to make
/// them; the front end therefore had a button of its own, and the operation did not
/// exist.
///
/// # What would have to break to make it turn red
///
/// Remove the operation from the registry, or make it refuse what `triangles`
/// already refuses - in which case the reported case comes back as it was.
#[test]
fn a_point_cloud_is_offered_to_the_triangulation() {
    let cloud = known(Geometry {
        kind: "x",
        patches: vec![patch(0, square(), Topology::Points)],
    });
    let registry = Registry::new();
    let offer = registry
        .offers(&cloud)
        .into_iter()
        .find(|o| o.operation.name() == "triangulate")
        .expect("offered");
    assert_eq!(offer.fitness, Fitness::Fit);

    // And `triangles` still refuses - that is the distinction, not an
    // oversight: copying a topology and deciding one are two gestures.
    let other = registry
        .offers(&cloud)
        .into_iter()
        .find(|o| o.operation.name() == "triangles")
        .unwrap();
    assert!(
        matches!(other.fitness, Fitness::Unfit(_)),
        "`triangles` makes no topology"
    );
}

/// What comes out is the surface, and the object it will become.
#[test]
fn the_triangulation_returns_a_surface_and_its_triangles() {
    let cloud = known(Geometry {
        kind: "x",
        patches: vec![patch(0, square(), Topology::Points)],
    });
    let plan = Registry::new()
        .plan(
            "triangulate",
            &cloud,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();

    // The computed shape: a square makes two triangles.
    let shape = &plan.shapes[0];
    let Topology::Triangles(triangles) = &shape.geometry.patches[0].topology else {
        panic!("triangles")
    };
    assert_eq!(triangles.len(), 2, "four corners make two triangles");
    assert_eq!(shape.geometry.patches[0].points.len(), 4);

    // And the writing is that of `triangles`, signed with the name of this one - a
    // second writer of the same object would be a second place for the same
    // mistakes.
    let object = plan.objects.iter().find(|o| o.role == "representation").unwrap();
    assert_eq!(object.qualified_type, "resqml22.TriangulatedSetRepresentation");
    assert_eq!(plan.activity.recipe, "triangulate");
    assert!(plan.activity.parameters.iter().any(
        |p| matches!(p, energyml_domain::provenance::Parameter::Text { title, value } if title == "how" && value == "delaunay-xy")
    ));
}

/// **An already triangulated surface is sent back to `triangles`, by name.**
///
/// Triangulating it again would lay a second skin over the first - the rule the
/// front end already held for its button. And a refusal that does not say where to go reads
/// as *this cannot be done*.
#[test]
fn an_already_triangulated_surface_is_sent_to_the_other_operation() {
    let surface = known(Geometry {
        kind: "x",
        patches: vec![patch(
            0,
            square(),
            Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
        )],
    });
    let registry = Registry::new();
    let offer = registry
        .offers(&surface)
        .into_iter()
        .find(|o| o.operation.name() == "triangulate")
        .unwrap();
    let Fitness::Unfit(energyml_build::operation::Refusal { message: why, .. }) = offer.fitness else {
        panic!("refused")
    };
    assert!(why.contains("`triangles`"), "{why}");
    assert!(why.contains("second skin"), "{why}");
}

/// **Collinear points have no triangulation, and that is said.**
///
/// It is the case that most looks like a success: the Delaunay succeeds and returns
/// zero triangles. An empty plan would read as *nothing to do here*.
#[test]
fn collinear_points_are_refused_rather_than_returned_empty() {
    let line = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 5.0], [2.0, 0.0, 9.0], [3.0, 0.0, 1.0]];
    let aligned = known(Geometry {
        kind: "x",
        patches: vec![patch(0, line, Topology::Points)],
    });
    // The offer is good - there are well over three points - and it is the computation
    // that refuses, which is the only honest answer: collinearity can only
    // be established by triangulating.
    let registry = Registry::new();
    let offer = registry
        .offers(&aligned)
        .into_iter()
        .find(|o| o.operation.name() == "triangulate")
        .unwrap();
    assert_eq!(offer.fitness, Fitness::Fit);
    let why = Registry::new()
        .plan(
            "triangulate",
            &aligned,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("refused");
    assert!(why.message().to_lowercase().contains("collinear"), "{why}");
}

/// Duplicates are counted, not refused - and the plan **says so**.
///
/// Two points at the same (x, y) are a single vertex for the Delaunay; the second
/// stays in the object written and is named by no triangle. Keeping quiet would make
/// a cloud with duplicates a surface that looks complete.
#[test]
fn nodes_that_no_triangle_names_are_counted() {
    let mut points = square();
    // The same (x, y) as the first corner, another altitude.
    points.push([0.0, 0.0, 42.0]);
    let cloud = known(Geometry {
        kind: "x",
        patches: vec![patch(0, points, Topology::Points)],
    });
    let plan = Registry::new()
        .plan(
            "triangulate",
            &cloud,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let note = plan.says.iter().find(|n| n.key == "nodes-unused").expect("said");
    assert!(note.detail.contains("1 of 5"), "{}", note.detail);
}

// -- isocontours -------------------------------------------------------------

/// A pyramid: four triangles around a vertex, hence a real relief to
/// contour. The vertex is at z = 10, the four corners at z = 0.
fn pyramid() -> Geometry {
    let points = vec![
        [0.0, 0.0, 0.0],
        [10.0, 0.0, 0.0],
        [10.0, 10.0, 0.0],
        [0.0, 10.0, 0.0],
        [5.0, 5.0, 10.0],
    ];
    Geometry {
        kind: "x",
        patches: vec![patch(
            0,
            points,
            Topology::Triangles(vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]]),
        )],
    }
}

/// **Contour lines are an operation**, and no longer a button of the front end.
///
/// The need: they should rather be only in the
/// list of possible operations.
#[test]
fn a_surface_is_offered_to_the_contour_lines() {
    let surface = known(pyramid());
    let registry = Registry::new();
    let offer = registry
        .offers(&surface)
        .into_iter()
        .find(|o| o.operation.name() == "isocontours")
        .expect("offered");
    assert_eq!(offer.fitness, Fitness::Fit);
}

/// **A cloud is sent back to `triangulate`, by name.** A contour
/// line is traced across faces; without triangles there is nothing to
/// cross, and a refusal that does not say where to go reads as *this cannot be done*.
#[test]
fn a_cloud_without_triangles_is_sent_back_to_the_triangulation() {
    let cloud = known(Geometry {
        kind: "x",
        patches: vec![patch(0, square(), Topology::Points)],
    });
    let registry = Registry::new();
    let offer = registry
        .offers(&cloud)
        .into_iter()
        .find(|o| o.operation.name() == "isocontours")
        .unwrap();
    let Fitness::Unfit(energyml_build::operation::Refusal { message: why, .. }) = offer.fitness else {
        panic!("refused")
    };
    assert!(why.contains("`triangulate`"), "{why}");
}

/// **The test that counts: closure comes from the chaining, never from a choice.**
///
/// A curve that goes around the summit of a pyramid is **closed**; writing it
/// open, or writing all the curves open, gives polylines
/// of which nothing to the eye distinguishes the good ones - a ring missing one
/// edge in two hundred looks like a ring.
///
/// And the plan **carries its shape**, so the closure is there too:
/// the two halves are confronted.
#[test]
fn a_curve_that_goes_around_the_summit_is_written_closed() {
    let surface = known(pyramid());
    let plan = Registry::new()
        .plan(
            "isocontours",
            &surface,
            &Choices::new().set("count", 4),
            &Identity::derived(),
            &when(),
        )
        .unwrap();

    // The object written, and what it says of its closures.
    let object = plan.objects.iter().find(|o| o.role == "representation").unwrap();
    assert_eq!(object.qualified_type, "resqml22.PolylineSetRepresentation");
    let patch = &object.json["LinePatch"][0];
    let closed = &patch["ClosedPolylines"];
    let any_closed = match closed["Values"].as_array() {
        Some(values) => values.iter().any(|v| v.as_bool() == Some(true)),
        // A constant array: all closed, or all open.
        None => closed["Value"].as_bool() == Some(true),
    };
    assert!(any_closed, "a curve around a summit is closed: {closed}");

    // And the computed shape says the same - two statements of the same fact.
    let shape = &plan.shapes[0];
    assert!(
        shape.geometry.patches[0].closed.iter().any(|c| *c),
        "the shape too"
    );
}

/// The levels actually cut are **reported**, and they are not `count`
/// regular values: the module anchors them on zero so that a legend carries
/// round numbers. Rederiving them elsewhere would make them drift.
#[test]
fn the_cut_levels_are_reported_and_not_guessable() {
    let surface = known(pyramid());
    let plan = Registry::new()
        .plan(
            "isocontours",
            &surface,
            &Choices::new().set("count", 4),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let note = plan.says.iter().find(|n| n.key == "levels").expect("said");
    assert!(!note.detail.is_empty(), "the levels are named");
    assert!(plan.activity.parameters.iter().any(
        |p| matches!(p, energyml_domain::provenance::Parameter::Double { title, .. } if title == "levelCount")
    ));
    // Where the values come from: the altitude here, for lack of a field in the bag.
    assert!(plan.activity.parameters.iter().any(
        |p| matches!(p, energyml_domain::provenance::Parameter::Text { title, value } if title == "contoured" && value == "the elevation")
    ));
}

/// **A flat surface is a refusal, not an empty plan.** It has a single
/// value, hence no interval to cut; returning an empty set of polylines would
/// read as a computation that failed without saying so.
#[test]
fn a_flat_surface_is_refused_saying_its_value() {
    let flat = known(Geometry {
        kind: "x",
        patches: vec![patch(
            0,
            square(),
            Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
        )],
    });
    let why = Registry::new()
        .plan(
            "isocontours",
            &flat,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("refused");
    assert!(why.message().contains("flat"), "{why}");
}

/// **`isocontours` already takes a field, over the elevation.** `field`
/// is declared on `IsocontoursInputs` since the operation was written, so
/// asking for a slot that is already there - what is missing is checked by the
/// two tests below. This one is the positive control: a field linear in `x`
/// (`value = x + 2`, so it does not land on the boundary values the square's
/// own edges already carry - `isocontour::levels` anchors on multiples of the
/// spacing from **zero**, and a field spanning exactly `0..10` would draw
/// spurious levels at 0 and 10, running along the square's own edges) is
/// contoured at its one level (`count = 1` over `2..12` gives the single
/// multiple of ten in range, level 10, so `x = 8`), and every point of the
/// resulting line lies on the analytic line `x = 8` - confronted
/// independently of the computation, on the written shape's own points.
#[test]
fn a_linear_field_is_contoured_on_the_analytic_line() {
    let pieces = known(Geometry {
        kind: "x",
        patches: vec![patch(
            0,
            square(),
            Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
        )],
    })
    .with(
        "phi",
        Source::Field {
            over: "shape".into(),
            on: "nodes".into(),
            values: vec![2.0, 12.0, 12.0, 2.0],
            uom: Some("m".into()),
        },
    );

    let plan = Registry::new()
        .plan(
            "isocontours",
            &pieces,
            &Choices::new().set("count", 1),
            &Identity::derived(),
            &when(),
        )
        .unwrap();

    assert!(plan.activity.parameters.iter().any(
        |p| matches!(p, energyml_domain::provenance::Parameter::Text { title, value } if title == "contoured" && value == "a field of the bag")
    ));
    let shape = &plan.shapes[0];
    let points: Vec<&[f64; 3]> = shape
        .geometry
        .patches
        .iter()
        .flat_map(|p| p.points.iter())
        .collect();
    assert!(!points.is_empty(), "a level was cut");
    for p in &points {
        assert!((p[0] - 8.0).abs() < 1e-9, "point {p:?} is not on x = 8");
    }
}

/// **A field on cells is refused rather than misread as one value per
/// node.** `field_of` would otherwise hand `isocontour::contour` two values
/// for two triangles as if they were four values at four nodes - the wrong
/// numbers on the wrong points, silently, since nothing here checks the
/// count against the node count. No transfer from cells to nodes exists in
/// this workspace, so this is a refusal, not a guess.
///
/// To break: drop the `field.on != "nodes"` check in `isocontours::check` -
/// this goes green on a bag no other test in this file exercises.
#[test]
fn a_field_on_the_cells_is_refused_rather_than_measured_on_nodes() {
    let pieces = known(Geometry {
        kind: "x",
        patches: vec![patch(
            0,
            square(),
            Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]]),
        )],
    })
    .with(
        "phi",
        Source::Field {
            over: "shape".into(),
            on: "cells".into(),
            values: vec![1.0, 2.0],
            uom: None,
        },
    );

    let why = Registry::new()
        .plan(
            "isocontours",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("a field on cells is refused");
    assert!(
        why.message().contains("cells") && why.message().contains("nodes"),
        "{why}"
    );
}

/// **The lattice written and the shape carried agree, point by point.**
///
/// The operation writes a `Point3dLatticeArray` - an origin, two directions,
/// two steps - and carries the developed points beside it. They are two
/// statements of one fact, separated by a reflection boundary that no shared
/// code crosses: the plan's development is arithmetic on
/// numbers, the reader's goes through `Reflect` and handles what producers
/// write. This repository's defence for that shape is to **confront
/// confronter**.
///
/// # What would have to break to make it turn red
///
/// An inverted axis, one step taken for the other, the origin placed at the centre rather
/// than at node (0, 0) - and the most dangerous, the z sheet read by columns
/// rather than by rows: the grid then comes out transposed, which no count of
/// points signals.
#[cfg(feature = "hdf5")]
#[test]
fn a_written_lattice_reads_back_as_the_shape_it_carried() {
    use std::sync::Arc;

    // A frankly asymmetric sheet: 3 × 2, different steps, and all distinct
    // heights. A square grid with regular z would read back
    // the same even if transposed.
    let lattice = Lattice {
        origin: [100.0, 200.0],
        spacing: [10.0, 25.0],
        counts: [3, 2],
        z: vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
    };
    let pieces = Pieces::empty().with("nappe", Source::Lattice(lattice)).with(
        "object",
        Source::Object(Known {
            reference: Some(Ref::new(
                "bbbbbbbb-0000-4000-8000-000000000002",
                "resqml22.Grid2dRepresentation",
                "The source",
            )),
            crs: Some(crs_ref()),
            vertical_uom: Some("m".into()),
            ..Default::default()
        }),
    );
    let plan = Registry::new()
        .plan("grid", &pieces, &Choices::new(), &Identity::derived(), &when())
        .unwrap();
    let carried = &plan.shapes[0].geometry.patches[0];
    assert_eq!(carried.points.len(), 6);

    let dir = scratch("grid-field");
    let out = dir.join("grid.epc");
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "grid.epc", registry.clone()).unwrap();
    store.external_files_mut().set_root(&dir);
    let made = energyml_build::apply::apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    store
        .save_as(&out, &energyml_storage::SaveOptions::default())
        .unwrap();

    let reread = energyml_storage::EpcStore::open(&out, registry).unwrap();
    let summary = reread
        .find_by_uuid(&plan.objects[0].uuid)
        .cloned()
        .expect("the object is there");
    let object = reread.get_object(&summary.uri).expect("it reads");
    let built = energyml_geometry::scene::object_of(object.as_reflect(), &reread, &summary.uri)
        .expect("and so does its geometry");
    let read = &built.geometry.patches[0];

    // The two halves, point by point. To the millimetre: they are the same
    // numbers passed through an external f64, not a computation redone another way.
    assert_eq!(read.points.len(), carried.points.len(), "the node count");
    for (at, (a, b)) in read.points.iter().zip(carried.points.iter()).enumerate() {
        for axis in 0..3 {
            assert!(
                (a[axis] - b[axis]).abs() < 1e-6,
                "node {at}, axis {axis}: the file says {}, the shape carried {}",
                a[axis],
                b[axis]
            );
        }
    }
    // And the topology, which says in which direction the sheet is read.
    assert_eq!(read.topology, carried.topology);
    let _ = std::fs::remove_dir_all(&dir);
}
