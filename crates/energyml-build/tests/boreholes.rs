//! `sigeol-boreholes`, on the shape the BRGM writes.
//!
//! The fixture is `Borehole_indent_extract_1_well.json` — one real hole,
//! `BSS000LKCG`, trimmed to four of its nine intervals — and the assertions
//! follow the root rule: **their form comes from the specification, their
//! values from the file.** Nothing here pins *1 000 boreholes*; what is pinned
//! is that a repeated depth range makes one node, that the uuid written is the
//! producer's own, that a vertical hole comes out as RESQML's vertical line.
//!
//! What would have to break for each to go red is said on each.

use serde_json::Value;

use energyml_build::geojson;
use energyml_build::operation::{Choices, Identity, Registry, When};
use energyml_build::pieces::{Pieces, Source};

fn when() -> When {
    When {
        creation: "2026-09-18T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// One borehole, in the shape of the real extract.
///
/// Four intervals over three distinct depth ranges: `(0, 0.5)`, `(0.5, 6.4)`
/// twice — two geologic units mapped onto one slice, as the export does 22 117
/// times — and `(6.4, 9.7)`.
fn export() -> String {
    let interval = |id: &str, begin: f64, end: f64, collection: &str| {
        format!(
            r#"{{"type":"Feature","id":"{id}",
                 "geometry":{{"type":"GeometryCollection","geometries":[]}},
                 "properties":{{
                   "@featureType":"BoreholeInterval",
                   "identifier":"https://data.geoscience.fr/id/BoreholeInterval/{id}",
                   "specification":{{"@href":"https://data.geoscience.fr/id/GeologicUnit/{id}"}},
                   "mappedIntervalBegin":{{"Quantity":{{"value":{begin},"@uom":"m"}}}},
                   "mappedIntervalEnd":{{"Quantity":{{"value":{end},"@uom":"m"}}}},
                   "collectionIdentifier":{{"value":"{collection}","@codeSpace":"BRGM"}},
                   "parentBorehole":{{"@href":"https://data.geoscience.fr/id/Borehole/BSS000LKCG"}}}}}}"#
        )
    };
    format!(
        r#"{{"type":"FeatureCollection","features":[
            {{"type":"Feature","id":"BSS000LKCG",
              "geometry":{{"type":"Point","coordinates":[2.32,48.9682]}},
              "properties":{{
                "@featureType":"Borehole",
                "identifier":"https://data.geoscience.fr/id/Borehole/BSS000LKCG",
                "description":"EPOS Borehole",
                "indexData":{{"BoreholeDetails":{{
                   "dateOfDrilling":{{"endPosition":"1965-11-20Z"}},
                   "boreholeLength":{{"Quantity":{{"value":15,"@uom":"m"}}}}}}}},
                "downholeDrillingDetails":[{{
                   "intervalBegin":{{"Quantity":{{"value":0,"@uom":"m"}}}},
                   "intervalEnd":{{"Quantity":{{"value":15,"@uom":"m"}}}}}}],
                "referenceLocation":{{"type":"Feature","id":"origin_position.2792603",
                   "geometry":{{"type":"Point","coordinates":[2.32,48.9682]}},
                   "properties":{{"@featureType":"OriginPosition","elevation":38.5}}}},
                "logElement":[{},{},{},{}],
                "parameter":[
                  {{"name":{{"@title":"parameter_id_forage"}},"value":"3ca79312-f3fa-4959-bae0-311c7a5c7359"}},
                  {{"name":{{"@title":"parameter_id_bss"}},"value":"BSS000LKCG"}},
                  {{"name":{{"@title":"parameter_indice_bss"}},"value":"01536X0058"}}]}}}}]}}"#,
        interval(
            "6872447",
            0.0,
            0.5,
            "BSS000LKCG_38325d6d-f932-4164-82dc-c1fbfebf7563"
        ),
        interval(
            "6872444",
            0.5,
            6.4,
            "BSS000LKCG_29d030fc-45f2-4841-924d-7929992a2f16"
        ),
        interval("6872446", 0.5, 6.4, "BSS000LKCG_GLU000AEKQ"),
        interval(
            "6872451",
            6.4,
            9.7,
            "BSS000LKCG_3888e211-0e34-4758-967c-2d15b223b988"
        ),
    )
}

fn pieces_of(text: &str) -> Pieces {
    let read = geojson::read(text).expect("a collection");
    // The `logElement`s are `GeometryCollection`s, which this reader refuses —
    // and they are **inside `properties`**, kept verbatim, so the borehole
    // itself reads. If that ever stopped being true the whole import would be
    // empty, so it is asserted here rather than assumed.
    assert!(read.refused.is_empty(), "{:?}", read.refused);
    Pieces::empty().with(
        "features",
        Source::Features {
            name: "Borehole_indent.json".into(),
            features: read.features,
        },
    )
}

fn planned(choices: Choices) -> energyml_build::operation::Plan {
    Registry::new()
        .plan(
            "sigeol-boreholes",
            &pieces_of(&export()),
            &choices,
            &Identity::derived(),
            &when(),
        )
        .expect("planned")
}

fn object<'a>(
    plan: &'a energyml_build::operation::Plan,
    role: &str,
) -> &'a energyml_build::operation::NewObject {
    plan.objects.iter().find(|o| o.role == role).unwrap_or_else(|| {
        panic!(
            "no `{role}` in {:?}",
            plan.objects.iter().map(|o| o.role.as_str()).collect::<Vec<_>>()
        )
    })
}

#[test]
fn a_borehole_import_into_degrees_is_refused_by_name() {
    // **The defect of 19/09, and it produced a package that was impeccable and
    // undrawable.** `crate::crs` already named it — *“the horizontal unit is
    // a degree while the vertical is a metre, which no reader can reconcile”*
    // — without ever telling the caller.
    //
    // For a borehole this is not a nuance: the object **is** a vertical
    // extent. The holes of a BRGM export are 25 m to 2,189 m deep over
    // a field 0.25° wide, so each comes out between a hundred and ten thousand times
    // longer than the model is wide. And nothing signalled it: `check`
    // passes, the MDs read back exactly, the map places each head where the
    // producer put it.
    //
    // Found on a package built in the front end, where `project` had simply been
    // left empty. On the 3D view the wells looked gigantic, whereas the same
    // operation run with `project` showed them fine.
    //
    // Refusing rather than projecting on a guess: an EPSG code chosen here
    // would move every coordinate by a decision that nobody made.
    // **Called directly**, without the registry — and that is deliberate. Through the
    // registry, `suggest` always fills `project` from the coordinates, and
    // the only paths that would leave it empty are already refused upstream:
    // an out-of-range latitude and a null geometry are set aside by the
    // GeoJSON reader, a field emptied by the validation of the parameter.
    //
    // This refusal therefore guards the **library**, not the form: a caller
    // who builds their parameters themselves can set `project: None`, and they
    // must not get a package in degrees. Testing it here is the only place
    // from which it is reachable — and a check that cannot fail would
    // prove nothing.
    use energyml_build::declare::Epsg;
    use energyml_build::operation::Operation;
    let collection = geojson::read(&export()).expect("a collection");
    let why = energyml_build::ops::sigeol_boreholes::SigeolBoreholes
        .plan(
            energyml_build::ops::sigeol_boreholes::BoreholeInputs {
                features: vec![energyml_build::declare::Features {
                    name: "features".into(),
                    file: "Borehole_indent.json".into(),
                    features: collection.features,
                }],
            },
            energyml_build::ops::sigeol_boreholes::BoreholeParams {
                source_epsg: Epsg(4326),
                project: None,
                vertical: None,
                data_file: "data.h5".into(),
            },
            &energyml_build::operation::Context {
                identity: &Identity::derived(),
                when: &when(),
                purpose: energyml_build::operation::Purpose::Writing,
                feedback: &energyml_build::typed::Silent,
                bag: &Pieces::empty(),
            },
        )
        .expect_err("degrees horizontally, metres vertically");
    let said = why.to_string();
    assert!(said.contains("4326"), "it names the system refused: {said}");
    assert!(said.contains("project"), "and the parameter to fill: {said}");

    // And the ordinary path still passes — otherwise this check would pass by
    // refusing everything, which is the fault one notch further.
    let plan = planned(Choices::new().set("project", 2154));
    // The projected half, and the compound over it that the holes name — role
    // `crs` was the `ProjectedCrs` until 24/09, a geometry naming a half
    // against RESQML TRG 2.2 §3.1 p. 22.
    assert_eq!(object(&plan, "projected-crs").json["$type"], "eml23.ProjectedCrs");
    assert_eq!(
        object(&plan, "crs").json["$type"],
        "eml23.LocalEngineeringCompoundCrs"
    );
}

#[test]
fn without_setting_anything_the_import_comes_out_in_metres_by_the_utm_zone_of_the_data() {
    // **The asymmetry found on 19/09 with `sigeol-units`**: that one fills a
    // slot from what it reads, this one filled none. `project`
    // therefore stayed empty in the form, the degrees were kept, and
    // each borehole came out between a hundred and ten thousand times longer than the model
    // is wide.
    //
    // It is a GeoJSON, so the EPSG codes should be known. The
    // **source** system is, RFC 7946 fixes it; the **target** is a
    // decision, and the UTM zone is the only proposal that follows from the numbers
    // rather than from a country.
    //
    // The shape comes from the rule — an import must come out in a single unit —
    // and the value from the file: 2.32 E falls in zone 31 north.
    let plan = planned(Choices::new());
    // The projected half by its role since 24/09 — `crs` is the compound the
    // holes name, over it.
    let crs = object(&plan, "projected-crs");
    assert_eq!(
        crs.json["$type"], "eml23.ProjectedCrs",
        "metric without anything being set"
    );
    assert_eq!(crs.json["AbstractProjectedCrs"]["EpsgCode"], 32_631);
    assert_eq!(
        object(&plan, "crs").json["$type"],
        "eml23.LocalEngineeringCompoundCrs"
    );
    // And the coordinates written are indeed metres, not degrees: it is
    // the fact on which everything else depends.
    let control = plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with("/controlPoints"))
        .expect("the points");
    let energyml_build::operation::Numbers::F64(values) = &control.values else {
        panic!("not floats")
    };
    assert!(values[0] > 100_000.0, "metres, not 2.32 degrees: {}", values[0]);
}

#[test]
fn when_every_hole_is_set_aside_the_refusal_names_why_and_how_many() {
    // **A wholesale refusal is not consumed.** The message said *“the plan
    // says why for each”* — and there is no plan: it is an error that is
    // returned, so the grouped reasons were thrown away and the caller
    // learned only that something was wrong.
    //
    // Found on 19/09 against an ordinary WFS answer from `data.geoscience.fr`,
    // whose boreholes carry no interval: thirty set aside, and
    // nothing to say so.
    //
    // The shape comes from the rule of the repository — an absence and a stupidity are not
    // the same news, so a refusal names what it saw — and the value
    // comes from the file.
    let bare = export().replace("\"logElement\"", "\"logElementAbsent\"");
    let why = Registry::new()
        .plan(
            "sigeol-boreholes",
            &pieces_of(&bare),
            &Choices::new().set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no hole can be written without a depth");
    let said = why.to_string();
    assert!(said.contains("set aside"), "{said}");
    assert!(said.contains("1 \u{d7} "), "the count of each reason: {said}");
    assert!(
        said.contains("logElement"),
        "the reason names the field it looked for: {said}"
    );
    // And it no longer points to a plan that does not exist.
    assert!(!said.contains("the plan says"), "{said}");
}

#[test]
fn a_hole_set_aside_is_named_not_only_counted() {
    // Lutetia keeps a BSS borehole with no elevation **to show its refusal by
    // name**; the first run of its recipe
    // said “no `referenceLocation.elevation`” with no hole in it — one of
    // twenty-three, and nothing to say which (24/09). The title is the BSS code
    // (`the_uuid_written_is_the_producers_own_and_the_title_its_bss_code`).
    let unknown_ground = export().replace("\"elevation\":38.5", "\"elevationAbsent\":38.5");
    let why = Registry::new()
        .plan(
            "sigeol-boreholes",
            &pieces_of(&unknown_ground),
            &Choices::new().set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect_err("a hole whose ground is unknown cannot be written");
    let said = why.to_string();
    assert!(said.contains("referenceLocation.elevation"), "the reason: {said}");
    assert!(said.contains("BSS000LKCG"), "the hole it set aside: {said}");
}

#[test]
fn one_borehole_makes_the_five_objects_and_the_pair_of_systems() {
    let plan = planned(Choices::new().set("project", 2154));
    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    // The frame once for the import — the two systems and, since 24/09, the
    // plane and the compound the geometries name (RESQML TRG 2.2 §3.1 p. 22) —
    // then the hole: the feature, its interpretation, the datum the depths are
    // measured from, the trajectory and the frame on it. A sixth object for the
    // hole would be one nobody asked for.
    assert_eq!(
        roles,
        vec![
            "projected-crs",
            "vertical-crs",
            "local-2d-crs",
            "crs",
            "feature",
            "interpretation",
            "datum",
            "trajectory",
            "frame"
        ]
    );
    // The datum sits in the compound (decision (b) of 24/09, common TRG v2.3
    // §13.25 p. 371), and the trajectory names the same compound.
    let frame = object(&plan, "crs");
    let datum = object(&plan, "datum");
    assert_eq!(
        datum.qualified_type,
        "eml23.ReferencePointInALocalEngineeringCompoundCrs"
    );
    assert_eq!(datum.json["Crs"]["Uuid"], frame.uuid.as_str());
    assert_eq!(
        object(&plan, "trajectory").json["Geometry"]["LocalCrs"]["Uuid"],
        frame.uuid.as_str()
    );
    // Three arrays: the control points, their parameters, the frame's node MDs.
    assert_eq!(plan.datasets.len(), 3);
    assert!(plan.dropped.is_empty(), "{:?}", plan.dropped);
}

#[test]
fn the_uuid_written_is_the_producers_own_and_the_title_its_bss_code() {
    // The decision: `parameter_id_forage` as the uuid, `parameter_id_bss` as
    // the title. The uuid is taken **verbatim**, not derived: a consumer
    // holding the BRGM's own key finds the object by it, which is the whole
    // value of a permanent identifier.
    let plan = planned(Choices::new().set("project", 2154));
    let feature = object(&plan, "feature");
    assert_eq!(feature.uuid, "3ca79312-f3fa-4959-bae0-311c7a5c7359");
    assert_eq!(feature.title, "BSS000LKCG");
    assert_eq!(feature.json["Citation"]["Title"], "BSS000LKCG");
    // And the others hang off it rather than repeating it: five objects with
    // one uuid would be a package that cannot address any of them.
    let uuids: std::collections::BTreeSet<&str> = plan.objects.iter().map(|o| o.uuid.as_str()).collect();
    assert_eq!(uuids.len(), plan.objects.len());
}

#[test]
fn a_second_run_of_the_same_file_writes_the_same_package() {
    // The reason every name is derived: an export is re-pulled, and a run that
    // renamed everything would make a diff say that all the data moved.
    let first = planned(Choices::new().set("project", 2154));
    let second = planned(Choices::new().set("project", 2154));
    let names = |plan: &energyml_build::operation::Plan| {
        plan.objects
            .iter()
            .map(|o| (o.role.clone(), o.uuid.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&first), names(&second));
}

#[test]
fn the_dates_go_where_the_producer_put_them() {
    let plan = planned(Choices::new().set("project", 2154));
    for role in ["feature", "interpretation", "trajectory", "frame", "datum"] {
        let citation = &object(&plan, role).json["Citation"];
        // When the hole was drilled — widened from a date to midnight UTC,
        // because EML's `Creation` is a date-time and the export writes a day.
        assert_eq!(citation["Creation"], "1965-11-20T00:00:00Z", "{role}");
        // When this package was written.
        assert_eq!(citation["LastUpdate"], "2026-09-18T00:00:00Z", "{role}");
    }
    // The widening is said, not done quietly: a date that gained a time of day
    // nobody stated is a precision the file does not have.
    assert!(
        plan.says.iter().any(|note| note.key == "dates-widened"),
        "{:?}",
        plan.says
    );
    // No `purpose` on this hole, so the file's own description stands.
    assert_eq!(
        object(&plan, "feature").json["Citation"]["Description"],
        "EPOS Borehole"
    );
}

#[test]
fn a_purpose_becomes_the_description_and_pushes_the_blurb_out() {
    // The decision: put the purpose in the
    // Citation.Description instead. `EPOS Borehole` is the collection's name, not
    // the hole's, so a real purpose replaces it rather than joining it — two
    // sources in one sentence would read as one statement the producer never
    // made.
    let text = export().replace(
        r#""dateOfDrilling""#,
        r#""purpose":[{"purpose":[{"@title":"EXPLOITATION/GEOTHERMIE/BASSE-ENERGIE","@href":"https://data.geoscience.fr/ncl/FunMaMa/11"}]}],"dateOfDrilling""#,
    );
    let plan = Registry::new()
        .plan(
            "sigeol-boreholes",
            &pieces_of(&text),
            &Choices::new().set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect("planned");
    assert_eq!(
        object(&plan, "feature").json["Citation"]["Description"],
        "EXPLOITATION/GEOTHERMIE/BASSE-ENERGIE"
    );
}

#[test]
fn a_hole_with_no_drilling_date_falls_back_on_the_run_and_is_counted() {
    // 95 of the 1 000 carry no date. Silently dating them today would make the
    // package claim they were drilled the day it was written.
    let text = export().replace(r#""dateOfDrilling":{"endPosition":"1965-11-20Z"},"#, "");
    let plan = Registry::new()
        .plan(
            "sigeol-boreholes",
            &pieces_of(&text),
            &Choices::new().set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect("planned");
    assert_eq!(
        object(&plan, "feature").json["Citation"]["Creation"],
        "2026-09-18T00:00:00Z"
    );
    let note = plan
        .says
        .iter()
        .find(|n| n.key == "no-drilling-date")
        .expect("counted");
    assert!(note.detail.contains('1'), "{}", note.detail);
}

#[test]
fn a_repeated_depth_range_makes_one_frame_node_and_not_two() {
    // Two `logElement`s state `(0.5, 6.4)`. Counting them would put two frame
    // nodes at the same measured depth, with a zero-length interval between —
    // a frame that reads as having a slice of hole of no thickness.
    let plan = planned(Choices::new().set("project", 2154));
    let frame = object(&plan, "frame");
    assert_eq!(frame.json["NodeCount"], 4);
    let nodes = plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with("/nodeMd"))
        .expect("the node MDs");
    match &nodes.values {
        energyml_build::operation::Numbers::F64(values) => assert_eq!(values, &[0.0, 0.5, 6.4, 9.7]),
        other => panic!("the node MDs are not floating point: {other:?}"),
    }
    // And the trajectory's control points are those same depths: the file
    // states nothing between them, so a knot between them would be invented.
    assert_eq!(
        frame.json["NodeCount"],
        object(&plan, "trajectory").json["Geometry"]["KnotCount"]
    );
}

#[test]
fn the_head_is_the_wellhead_and_the_elevation_is_the_ground() {
    // No `project`, so the degrees stand as they are and the numbers can be
    // read straight: the hole is at its own coordinates, and each control
    // point is that place at `elevation − md`.
    let plan = planned(Choices::new().set("project", 2154));
    let control = plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with("/controlPoints"))
        .expect("the control points");
    let energyml_build::operation::Numbers::F64(values) = &control.values else {
        panic!("the control points are not floating point")
    };
    assert_eq!(values.len(), 4 * 3);
    // **The shape comes from the spec, the values from the file**: a vertical hole
    // has the *same* horizontal at each node, and that is what is pinned. The
    // exact pair is the one that Lambert-93 gives at 2.32 / 48.9682, and it is
    // confronted with the producer elsewhere — by an inverse written from the
    // EPSG formulas, not by a number copied here.
    let (head_x, head_y) = (values[0], values[1]);
    assert!(
        head_x > 100_000.0,
        "projected, hence in metres and not in degrees: {head_x}"
    );
    #[allow(clippy::needless_range_loop)]
    for knot in 0..4 {
        assert_eq!(values[knot * 3], head_x, "every knot is under the head");
        assert_eq!(values[knot * 3 + 1], head_y);
    }
    assert_eq!(&values[2..3], &[38.5], "the first knot is at the ground");
    assert_eq!(
        values[11],
        38.5 - 9.7,
        "the last is the deepest measured depth below it"
    );

    // The datum sits at the head, at the ground, and says what it is.
    let datum = object(&plan, "datum");
    assert_eq!(datum.json["VerticalCoordinate"], 38.5);
    assert_eq!(datum.json["Kind"], "ground level");
    assert_eq!(datum.json["HorizontalCoordinates"]["Coordinate1"], head_x);
}

#[test]
fn a_vertical_hole_collared_above_the_zero_is_a_linear_spline_and_its_parameters_are_the_mds() {
    // Nothing here declares the hole vertical: the shared writer measures it
    // off the control points it is handed. That is what makes the delegation
    // worth it — one detector, and the SIGEOL holes go through the same one
    // `well-from-polyline` does.
    //
    // **And vertical is not enough to take RESQML's kind 0.** That kind reads
    // its parameter as a depth through the CRS, so it only says what the points
    // say when the datum is at the system's zero. A BRGM borehole is collared
    // at 38.5 m, so writing kind 0 sank every frame node by that much: the
    // trajectory drawn where it belongs, and its frame thirty-eight metres
    // under it. The need, on the thousand imported holes: everything
    // works, even the addition to the 3D scene. At least in appearance, because nothing
    // shows in the canvas.
    let plan = planned(Choices::new().set("project", 2154));
    assert_eq!(object(&plan, "trajectory").json["Geometry"]["LineKindIndex"], 1);
    assert!(
        plan.says.iter().any(|note| note.key == "vertical-datum-offset"),
        "{:?}",
        plan.says
    );

    // **The parameters are the measured depths**, which is what the frame's
    // `NodeMd` indexes into — the same numbers as `MdInterval`, below.
    let parameters = plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with("/controlPointParameters"))
        .expect("the parameters");
    let energyml_build::operation::Numbers::F64(values) = &parameters.values else {
        panic!("not floating point")
    };
    assert_eq!(values[0], 0.0, "the first knot is the wellhead, at MD zero");
    assert_eq!(values[3], 9.7, "the last is the deepest measured depth");
}

#[test]
fn the_trajectory_states_its_md_interval_in_the_files_own_unit() {
    let plan = planned(Choices::new().set("project", 2154));
    let interval = &object(&plan, "trajectory").json["MdInterval"];
    assert_eq!(interval["MdMin"], 0.0);
    assert_eq!(interval["MdMax"], 9.7);
    // Read from the intervals, never defaulted: a hole in feet read as metres
    // is a hole three times too short, and it opens cleanly.
    assert_eq!(interval["Uom"], "m");
    assert!(
        interval["Datum"]["Uuid"].is_string(),
        "the interval names the datum it is measured from"
    );
}

#[test]
fn the_producers_identifiers_survive_as_aliases_with_their_authority() {
    let plan = planned(Choices::new().set("project", 2154));
    let aliases = object(&plan, "feature").json["Aliases"]
        .as_array()
        .expect("aliases")
        .clone();
    let by_identifier = |wanted: &str| -> Value {
        aliases
            .iter()
            .find(|a| a["Identifier"] == wanted)
            .unwrap_or_else(|| panic!("no alias `{wanted}` in {aliases:?}"))
            .clone()
    };
    // An alias without its authority resolves to nothing: `BSS000LKCG` means
    // one thing in the BRGM's register and nothing anywhere else.
    assert_eq!(
        by_identifier("BSS000LKCG")["IdentifierKind"],
        "regulatory identifier"
    );
    assert_eq!(by_identifier("BSS000LKCG")["authority"], "BRGM/BSS");
    assert_eq!(by_identifier("01536X0058")["IdentifierKind"], "industry code");
    assert_eq!(
        by_identifier("https://data.geoscience.fr/id/Borehole/BSS000LKCG")["IdentifierKind"],
        "unique identifier"
    );
    for alias in &aliases {
        let identifier = alias["Identifier"].as_str().expect("a string");
        // The schema caps it at 64 characters, and half a URL resolves to
        // nothing while looking exactly like a whole one.
        assert!(identifier.chars().count() <= 64, "{identifier}");
    }
}

#[test]
fn what_the_export_measures_about_the_hole_is_kept_with_its_unit() {
    let plan = planned(Choices::new().set("project", 2154));
    let extensions = object(&plan, "feature").json["ExtensionNameValue"]
        .as_array()
        .expect("extensions");
    let named = |wanted: &str| {
        extensions
            .iter()
            .find(|e| e["Name"] == wanted)
            .unwrap_or_else(|| panic!("no `{wanted}`"))
    };
    assert_eq!(named("boreholeLength")["Value"]["_"], "15");
    // The unit travels with the number. A length with no unit is a number.
    assert_eq!(named("boreholeLength")["Value"]["uom"], "m");
    assert_eq!(named("drilledIntervalEnd")["Value"]["_"], "15");
    // Four `logElement`s over three depth ranges: the count is of what was
    // read, so that the collapse to three is traceable rather than invisible.
    assert_eq!(named("sigeolIntervalCount")["Value"]["_"], "4");
}

#[test]
fn the_interpretation_says_the_hole_was_drilled_and_points_at_its_feature() {
    let plan = planned(Choices::new().set("project", 2154));
    let interpretation = object(&plan, "interpretation");
    assert_eq!(interpretation.json["Domain"], "depth");
    // The export states a drilled interval and a date of drilling: this hole
    // exists in the ground, which `well-from-polyline` cannot say of a line
    // somebody drew.
    assert_eq!(interpretation.json["IsDrilled"], true);
    assert_eq!(
        interpretation.json["InterpretedFeature"]["Uuid"],
        "3ca79312-f3fa-4959-bae0-311c7a5c7359"
    );
    // A hole in the national register is well known, which is the one place
    // this workspace can honestly say true.
    assert_eq!(object(&plan, "feature").json["IsWellKnown"], true);
    // Both representations represent the interpretation, not the feature.
    for role in ["trajectory", "frame"] {
        assert_eq!(
            object(&plan, role).json["RepresentedObject"]["Uuid"],
            interpretation.uuid,
            "{role}"
        );
    }
}

#[test]
fn reprojecting_moves_the_head_and_leaves_the_depths_alone() {
    // Lambert-93. The depths are along the hole and have nothing to do with the
    // horizontal system; the head does. A reprojection that changed the MDs
    // would be the bug this separates.
    let plan = planned(Choices::new().set("project", serde_json::json!(2154)));
    let control = plan
        .datasets
        .iter()
        .find(|d| d.path.ends_with("/controlPoints"))
        .expect("points");
    let energyml_build::operation::Numbers::F64(values) = &control.values else {
        panic!("not floats")
    };
    // Paris in metres, not degrees — the diagnosis is the number itself.
    assert!(
        values[0] > 600_000.0 && values[0] < 700_000.0,
        "x = {}",
        values[0]
    );
    assert!(
        values[1] > 6_800_000.0 && values[1] < 6_900_000.0,
        "y = {}",
        values[1]
    );
    assert_eq!(object(&plan, "trajectory").json["MdInterval"]["MdMax"], 9.7);
    assert_eq!(plan.objects[0].qualified_type, "eml23.ProjectedCrs");
    // Placed under the identity compound, so the numbers above are Lambert-93
    // as they stand — the compound moves nothing.
    let frame = object(&plan, "crs");
    assert_eq!(frame.json["$type"], "eml23.LocalEngineeringCompoundCrs");
    assert_eq!(frame.json["OriginVerticalCoordinate"], 0.0);
    assert_eq!(
        object(&plan, "local-2d-crs").json["OriginProjectedCoordinate1"],
        0.0
    );
}

// -- what is refused, and by name ---------------------------------------------

#[test]
fn a_collection_with_no_borehole_is_refused_naming_what_it_held() {
    let text = r#"{"type":"FeatureCollection","features":[
        {"type":"Feature","id":"x","geometry":{"type":"Point","coordinates":[2.0,48.0]},
         "properties":{"@featureType":"GeologicUnit"}}]}"#;
    let why = Registry::new()
        .plan(
            "sigeol-boreholes",
            &pieces_of(text),
            &Choices::new().set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect_err("refused");
    assert!(format!("{why}").contains("Borehole"), "{why}");
}

#[test]
fn a_hole_with_no_ground_is_set_aside_by_name_and_the_others_go_on() {
    // The elevation is where the measured depths start. Assuming zero would put
    // a Paris-basin hole at sea level, and the package would open cleanly.
    let text = export().replace(r#""elevation":38.5"#, r#""elevation":null"#);
    let why = Registry::new()
        .plan(
            "sigeol-boreholes",
            &pieces_of(&text),
            &Choices::new().set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect_err("nothing left to write");
    assert!(format!("{why}").contains("set aside"), "{why}");
}

#[test]
fn a_hole_whose_depths_are_not_in_metres_is_set_aside_rather_than_converted() {
    // The frame's vertical axis is the metre (`crs::build`). Subtracting feet
    // from metres gives a hole three times too short in a package that opens
    // cleanly — so the unit is named and the conversion left to whoever owns
    // that decision.
    let text = export().replace(r#""@uom":"m""#, r#""@uom":"ft""#);
    // The replacement must have bitten, or the test would pass on the original.
    assert!(
        text.contains(r#""@uom":"ft""#) && !text.contains(r#""@uom":"m""#),
        "the fixture was not altered"
    );
    let why = Registry::new()
        .plan(
            "sigeol-boreholes",
            &pieces_of(&text),
            &Choices::new().set("project", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect_err("nothing left to write");
    assert!(format!("{why}").contains("set aside"), "{why}");
}

#[test]
fn given_uuids_are_refused_because_the_names_belong_to_the_producer() {
    // The same argument `sigeol-units` makes: these objects are named from the
    // BRGM's own identifiers so that a second import can be diffed against the
    // first. Drawn uuids would throw that away without a word.
    let why = Registry::new()
        .plan(
            "sigeol-boreholes",
            &pieces_of(&export()),
            &Choices::new(),
            &Identity::Given(vec!["11111111-1111-1111-1111-111111111111".into()]),
            &when(),
        )
        .expect_err("refused");
    assert!(format!("{why}").contains("producer"), "{why}");
}

// -- the objects must read back, which is the only proof the JSON is right ----

/// **Written into a package, and read back out of it.**
///
/// Everything above asserts on the plan, which is JSON this test module and the
/// operation both wrote — two halves of one opinion. This one hands the plan to
/// `apply`, which parses each object into the generated type and refuses what
/// does not fit: `Aliases`, `ExtensionNameValue`, `LastUpdate`, `Description`,
/// the enumerated alias kinds, the `StringMeasure`'s unit attribute. A typo in
/// any of those names is invisible to a JSON assertion and fatal here.
///
/// The arrays are left out (`apply_with(.., false)`): they need an HDF5 backend,
/// and what is under test is the objects.
#[test]
fn every_object_reads_back_into_its_generated_type() {
    use std::sync::Arc;

    let plan = planned(Choices::new().set("project", 2154));
    let schemas = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "boreholes.epc", schemas).expect("a new package");
    let made = energyml_build::apply::apply_with(&plan, &mut store, &when(), false);
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    // The seven objects, the activity template and the activity.
    assert_eq!(made.written.len(), plan.objects.len() + 2, "{:?}", made.written);

    // And the producer's identifiers really landed on the object, not merely in
    // a `Value` nobody parsed.
    let uri = store
        .find_by_uuid("3ca79312-f3fa-4959-bae0-311c7a5c7359")
        .expect("the wellbore feature is in the package")
        .uri
        .clone();
    let written = energyml_core::json::write_value(store.edit(&uri).expect("it parses"));
    let aliases = written["Aliases"]
        .as_array()
        .expect("aliases survived the round trip");
    assert_eq!(aliases.len(), 3, "{aliases:?}");
    assert_eq!(written["Citation"]["Creation"], "1965-11-20T00:00:00Z");
    assert_eq!(written["Citation"]["LastUpdate"], "2026-09-18T00:00:00Z");
    assert_eq!(written["ExtensionNameValue"][0]["Value"]["uom"], "m");
}
