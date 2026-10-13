//! The trait, put to the test from both ends.
//!
//! `crs` takes nothing and makes four objects; `sigeol-units` takes a feature
//! collection and makes many. Between the two, what the operations' specification promises: a
//! refusal is named, a missing choice is refused before any work, a
//! suggestion fills what was left out, the objects are named the same on two
//! runs, and the form an operation declares is the template the file holds.
//!
//! What would have to break for each to go red is said on each.

use std::sync::Arc;

use serde_json::json;

use energyml_build::apply::apply;
use energyml_build::geojson;
use energyml_build::operation::{Applies, Choices, Fitness, Identity, Otherwise, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_build::template;

fn when() -> When {
    When {
        creation: "2026-09-09T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// Three features in the shape the SIGEOL exports write, two on one place.
fn export() -> String {
    let feature = |unit: &str, lon: f64, lat: f64, z: f64, older: (&str, f64), younger: (&str, f64)| {
        format!(
            r#"{{"type":"Feature","geometry":{{"type":"Point","coordinates":[{lon},{lat},{z}]}},
               "properties":{{
                 "composition":[{{"material":{{"StratiRockMaterial":{{"lithostratigraphy":{{
                    "@href":"https://referentiel-lithostrat.brgm-rec.fr/#/lithostrat/search/details/{unit}",
                    "@title":"{unit}"}}}}}}}}],
                 "geologicHistory":[{{
                    "olderNamedAge":{{"@title":"{}"}},
                    "youngerNamedAge":{{"@title":"{}"}},
                    "numericAge":{{"NumericAgeRange":{{
                       "olderBoundDate":{{"Quantity":{{"value":{}}}}},
                       "youngerBoundDate":{{"Quantity":{{"value":{}}}}}}}}}}}]}}}}"#,
            older.0, younger.0, older.1, younger.1
        )
    };
    let a = feature(
        "Marnes et Caillasses",
        2.29,
        48.87,
        58.0,
        ("Bartonien", 37.8),
        ("Lutétien", 47.8),
    );
    let b = feature(
        "Marnes et Caillasses supérieures",
        2.40,
        48.90,
        50.0,
        ("Bartonien", 37.8),
        ("Bartonien", 41.2),
    );
    format!(r#"{{"type":"FeatureCollection","features":[{a},{a},{b}]}}"#)
}

fn sigeol_pieces() -> Pieces {
    let collection = geojson::read(&export()).expect("a collection");
    Pieces::empty().with(
        "export",
        Source::Features {
            name: "GeologicUnit_Bartonien_pretty.json".into(),
            features: collection.features,
        },
    )
}

// -- the registry, and what it offers ---------------------------------------

#[test]
fn an_empty_bag_offers_crs_and_refuses_sigeol_by_name() {
    // To break: `fitness` returning `Fit` for everything, or `offers` dropping
    // the unfit instead of listing them with their reason.
    let registry = Registry::new();
    let offers = registry.offers(&Pieces::empty());
    let crs = offers
        .iter()
        .find(|o| o.operation.name() == "crs")
        .expect("crs is offered");
    assert_eq!(crs.fitness, Fitness::Fit);
    let sigeol = offers
        .iter()
        .find(|o| o.operation.name() == "sigeol-units")
        .expect("listed, unfit");
    match &sigeol.fitness {
        Fitness::Unfit(why) => assert!(
            why.message.contains("feature collection") && why.message.contains("nothing"),
            "{why}"
        ),
        Fitness::Fit => panic!("sigeol cannot work on nothing"),
    }
}

#[test]
fn a_feature_collection_offers_sigeol_and_suggests_the_stage_from_the_file_name() {
    let registry = Registry::new();
    let pieces = sigeol_pieces();
    let offer = registry
        .offers(&pieces)
        .into_iter()
        .find(|o| o.operation.name() == "sigeol-units")
        .expect("offered");
    assert_eq!(offer.fitness, Fitness::Fit);
    let stage = offer
        .needs
        .iter()
        .find(|s| s.name == "stage")
        .expect("the stage slot");
    // The one fact that is not in the file, read off its name - and still a
    // slot, so that a file named otherwise asks for it.
    assert_eq!(stage.suggested, Some(json!("Bartonien")));
    assert!(stage.is_required());
}

#[test]
fn a_bag_of_the_wrong_kind_is_refused_naming_both_ends() {
    // A shape is not a feature collection, and the refusal says what it got.
    let pieces = Pieces::empty().with(
        "mesh",
        Source::Shape {
            geometry: energyml_geometry::geometry::Geometry {
                kind: "obj",
                patches: vec![],
            },
            closed: None,
        },
    );
    let why = Registry::new()
        .plan(
            "sigeol-units",
            &pieces,
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("refused");
    assert!(why.message().contains("shape"), "{why}");
}

// -- the choices ------------------------------------------------------------

// -- what a slot says about itself ------------------------------------------

/// The pieces a well is planned from: one polyline, and nothing known.
fn a_polyline() -> Pieces {
    use energyml_geometry::geometry::{Geometry, Patch, Topology};
    Pieces::empty().with(
        "line",
        Source::Shape {
            geometry: Geometry {
                kind: "polylines",
                patches: vec![Patch {
                    index: 0,
                    node_offset: 0,
                    points: vec![[0.0, 0.0, 0.0], [0.0, 0.0, -100.0], [0.0, 0.0, -200.0]],
                    topology: Topology::Polylines(vec![3]),
                    closed: Vec::new(),
                }],
            },
            closed: None,
        },
    )
}

#[test]
fn the_two_halves_of_the_frame_are_declared_alternatives() {
    // The feedback: a form asked for a crs **and** epsgCodes at once. What
    // fixes it is the declaration, and it is what is read here - a
    // form that hid things at random would pass this test, and a module that
    // excluded nothing would too. To make it turn red: remove a `when` from
    // `common_slots`.
    let registry = Registry::new();
    let needs = registry.get("pointset").unwrap().needs(&a_polyline());
    let of = |name: &str| needs.iter().find(|s| s.name == name).unwrap().applies.clone();
    assert_eq!(of("crs"), Applies::Without(&["sourceEpsg"]));
    assert_eq!(of("sourceEpsg"), Applies::Without(&["crs"]));
    assert_eq!(of("project"), Applies::With(&["sourceEpsg"]));
    assert_eq!(of("vertical"), Applies::With(&["sourceEpsg"]));
}

#[test]
fn an_inapplicable_slot_is_neither_required_nor_filled_and_giving_it_anyway_is_refused() {
    // Two halves, and the second is the one that bites: giving `project` beside
    // a `crs` of the package would reproject into a system that nobody
    // asked for. Letting it fall silently would be worse than refusing it.
    let registry = Registry::new();
    let choices = Choices::new()
        .set("crs", json!({ "$type": "eml23.DataObjectReference", "Uuid": "c-1", "QualifiedType": "eml23.LocalEngineeringCompoundCrs", "Title": "frame" }))
        .set("project", 2154)
        .set("title", "x");
    let why = registry
        .plan("pointset", &a_polyline(), &choices, &Identity::derived(), &when())
        .expect_err("both");
    // Named at both ends: the slot given, and the one without which it means
    // nothing. A `crs` of the package leaves `sourceEpsg` empty, so `project`
    // has nothing to reproject *from*.
    assert!(
        why.message().contains("`project`") && why.message().contains("`sourceEpsg`"),
        "{why}"
    );

    // And the other half, the exclusion itself: both frames together.
    let both = Choices::new()
        .set("crs", json!({ "$type": "eml23.DataObjectReference", "Uuid": "c-1", "QualifiedType": "eml23.LocalEngineeringCompoundCrs", "Title": "frame" }))
        .set("sourceEpsg", 4326)
        .set("title", "x");
    let why = registry
        .plan("pointset", &a_polyline(), &both, &Identity::derived(), &when())
        .expect_err("two frames");
    assert!(why.message().contains("alternatives"), "{why}");
}

#[test]
fn a_suggestion_suffices_to_satisfy_a_slot_that_depends_on_it() {
    // The asymmetry of the two halves of `Applies`, and the case that required it:
    // setting `project` alone while letting `sourceEpsg` take its suggested 4326
    // is ordinary. To make it turn red: read `With` on the caller's
    // answers only.
    use energyml_build::operation::Slot;
    let slots = vec![
        Slot::optional("sourceEpsg", energyml_build::operation::Need::Epsg, "").suggest(json!(4326)),
        Slot::optional("project", energyml_build::operation::Need::Epsg, "")
            .when(Applies::With(&["sourceEpsg"])),
    ];
    let settled = Choices::new()
        .set("project", 2154)
        .settle(&slots)
        .expect("the suggested one counts");
    assert_eq!(settled.integer("project"), Some(2154));
    assert_eq!(settled.integer("sourceEpsg"), Some(4326));
}

#[test]
fn an_empty_that_creates_says_so() {
    // The question was whether the interpretation could also be offered for
    // creation, with its feature, and the two linked - the operation already did, and nothing said so.
    // This test is what keeps the sentence: an `Otherwise::Nothing` here would make
    // the form read *you may do without*.
    let registry = Registry::new();
    let well = registry.get("well-from-polyline").unwrap();
    assert_eq!(
        well.represents(),
        Some(("resqml22.WellboreFeature", "resqml22.WellboreInterpretation"))
    );
    let needs = well.needs(&a_polyline());
    let interpretation = needs.iter().find(|s| s.name == "interpretation").unwrap();
    assert_eq!(
        interpretation.otherwise,
        Otherwise::Creates {
            types: vec!["resqml22.WellboreFeature", "resqml22.WellboreInterpretation"]
        }
    );
    let datum = needs.iter().find(|s| s.name == "datum").unwrap();
    // **Two forms since 24/09**, and the frame decides:
    // in the compound that a geometry names - common TRG v2.3 §13.25, p. 371 -
    // or, under a `ProjectedCrs` taken as it stands, in it and its vertical
    // (§13.24, p. 370). Declaring only one, the old one, promised the
    // form an object that the plan no longer writes under a compound.
    assert_eq!(
        datum.otherwise,
        Otherwise::Creates {
            types: vec![
                "eml23.ReferencePointInALocalEngineeringCompoundCrs",
                "eml23.ReferencePointInACrs"
            ]
        }
    );

    // And an operation that creates none does not claim to.
    let plain = registry.get("pointset").unwrap().needs(&a_polyline());
    let interpretation = plain.iter().find(|s| s.name == "interpretation").unwrap();
    assert_eq!(interpretation.otherwise, Otherwise::Nothing);
}

#[test]
fn an_activity_records_what_the_run_was_told_and_returns_it() {
    // **The round trip**, and it is what makes *redo like this
    // activity* more than a promise: the plan writes each setting chosen
    // under the name of its slot, and `from_activity` reads them back as they are.
    //
    // The suggestion counts as much as the answer: `projected` was not
    // given, it takes its default, and it is **that** value that a second run
    // should receive to come out the same. An activity that recorded only what
    // was typed would not be a provenance.
    //
    // To make it turn red: let each operation choose what it
    // records, which was the case before - the activity of a well said
    // `sourceEpsg` and not `ground`.
    let registry = Registry::new();
    let choices = Choices::new().set("epsg", 2154).set("title", "Lambert-93");
    let plan = registry
        .plan("crs", &Pieces::empty(), &choices, &Identity::derived(), &when())
        .expect("planned");

    let activity = json!({ "Parameter": plan
        .activity
        .parameters
        .iter()
        .map(|p| match p {
            energyml_domain::provenance::Parameter::Double { title, value, .. } =>
                json!({ "Title": title, "Value": value }),
            energyml_domain::provenance::Parameter::Text { title, value } =>
                json!({ "Title": title, "Value": value }),
            energyml_domain::provenance::Parameter::Object { title, reference } =>
                json!({ "Title": title, "DataObject": reference }),
        })
        .collect::<Vec<_>>() });

    let slots = registry.get("crs").unwrap().needs(&Pieces::empty());
    let read = energyml_build::exemplar::from_activity(&slots, &activity);
    let of = |name: &str| {
        read.iter()
            .find(|one| one.slot == name)
            .map(|one| one.value.clone())
    };
    // An integer, and not the `2154.0` that the parameter carries: see `coerce`.
    assert_eq!(of("epsg"), Some(json!(2154)));
    assert_eq!(of("title"), Some(json!("Lambert-93")));
    // A flag: eml 2.3 has none, so it travels as text and comes back
    // as a boolean - the conversion is in `coerce`, not in the caller.
    assert_eq!(
        of("projected"),
        Some(json!(true)),
        "a suggestion is an input of the run like any other"
    );
}

#[test]
fn a_required_slot_left_empty_is_refused_before_any_work() {
    // To break: `settle` filling a required slot with nothing, or `plan`
    // being called anyway.
    let why = Registry::new()
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no epsg");
    assert!(why.message().contains("`epsg` is required"), "{why}");
}

#[test]
fn a_suggestion_fills_what_was_left_out_and_a_wrong_shape_is_refused() {
    let registry = Registry::new();
    let plan = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 2154),
            &Identity::derived(),
            &when(),
        )
        .expect("projected by suggestion");
    assert_eq!(plan.objects[0].qualified_type, "eml23.ProjectedCrs");
    // And, `projected` suggested true, the frame a geometry names is the
    // compound over it - not the `ProjectedCrs`, which is what this line alone
    // said until 24/09 (RESQML TRG 2.2 §3.1, p. 22).
    assert_eq!(
        plan.of_role("crs")
            .next()
            .map(|frame| frame.qualified_type.as_str()),
        Some("eml23.LocalEngineeringCompoundCrs")
    );

    let why = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", "deux mille"),
            &Identity::derived(),
            &when(),
        )
        .expect_err("not a code");
    assert!(
        why.message().contains("`epsg`") && why.message().contains("EPSG code"),
        "{why}"
    );
}

// -- crs, the operation with no input ---------------------------------------

#[test]
fn crs_makes_the_two_halves_and_the_compound_that_joins_them_named_the_same_twice() {
    // To break: a uuid drawn from a clock or a random source.
    //
    // **Four objects over metres, since 24/09.** The test said two: the
    // `ProjectedCrs` and the `VerticalCrs`, and a geometry named the first -
    // against RESQML TRG 2.2 §3.1 p. 22, *“From: AbstractGeometry.LocalCrs -
    // To: LocalEngineeringCompoundCrs”*, and leaving the vertical referenced
    // by nothing. The two halves keep their
    // order and their names; the plane and the compound follow them.
    let registry = Registry::new();
    let choices = Choices::new().set("epsg", 2154).set("vertical", 5720);
    let once = registry
        .plan("crs", &Pieces::empty(), &choices, &Identity::derived(), &when())
        .unwrap();
    let twice = registry
        .plan("crs", &Pieces::empty(), &choices, &Identity::derived(), &when())
        .unwrap();
    let kinds: Vec<&str> = once.objects.iter().map(|o| o.qualified_type.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "eml23.ProjectedCrs",
            "eml23.VerticalCrs",
            "eml23.LocalEngineering2dCrs",
            "eml23.LocalEngineeringCompoundCrs"
        ]
    );
    for (a, b) in once.objects.iter().zip(&twice.objects) {
        assert_eq!(a.uuid, b.uuid, "{}", a.qualified_type);
    }
    assert_eq!(once.objects[0].json["Citation"]["Title"], json!("EPSG:2154"));
    assert_eq!(once.objects[0].json["Citation"]["Originator"], json!("test"));
    // The compound names both halves: the vertical directly, the projected
    // through the plane, which embeds it (CRS.xsd l. 504).
    let compound = &once.objects[3];
    assert_eq!(
        compound.role, "crs",
        "the frame a geometry names is the one role `crs` says"
    );
    assert_eq!(compound.json["VerticalCrs"]["Uuid"], json!(once.objects[1].uuid));
    assert_eq!(
        compound.json["LocalEngineering2dCrs"]["Uuid"],
        json!(once.objects[2].uuid)
    );
    assert_eq!(
        once.objects[2].json["OriginProjectedCrs"]["uuid"],
        json!(once.objects[0].uuid)
    );
    // Degrees, when asked - and 2154 is no geographic code, so the degrees are
    // asked of 4326. No compound over degrees (CRS.xsd l. 504): the two halves,
    // the geographic one named, and the plan says it is the named exception.
    let degrees = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 4326).set("projected", false),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let kinds: Vec<&str> = degrees
        .objects
        .iter()
        .map(|o| o.qualified_type.as_str())
        .collect();
    assert_eq!(kinds, ["eml23.Geographic2dCrs", "eml23.VerticalCrs"]);
    assert_eq!(degrees.objects[0].role, "crs");
    assert!(
        degrees.says.iter().any(|n| n.key == "frame-degrees"),
        "{:?}",
        degrees.says
    );
}

#[test]
fn given_uuids_are_taken_in_the_plans_order_and_too_few_is_refused() {
    // Four over metres, in the order the plan lists them. The test gave two
    // until 24/09, when `crs` wrote two objects; one short is still refused,
    // naming how many the plan needs.
    let registry = Registry::new();
    let choices = Choices::new().set("epsg", 2154);
    let four: Vec<String> = (1..=4).map(|n| format!("{n}{n}{n}{n}{n}{n}{n}{n}-{n}{n}{n}{n}-4{n}{n}{n}-8{n}{n}{n}-{n}{n}{n}{n}{n}{n}{n}{n}{n}{n}{n}{n}")).collect();
    let plan = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &choices,
            &Identity::Given(four.clone()),
            &when(),
        )
        .unwrap();
    for (at, (object, uuid)) in plan.objects.iter().zip(&four).enumerate() {
        assert_eq!(&object.uuid, uuid, "object {at}");
        assert_eq!(object.json["uuid"], json!(uuid), "object {at}");
    }
    let short = Identity::Given(four[..3].to_vec());
    let why = registry
        .plan("crs", &Pieces::empty(), &choices, &short, &when())
        .expect_err("one short");
    assert!(why.message().contains("at least 4"), "{why}");
    // Degrees take two, and two suffice.
    let two = Identity::Given(four[..2].to_vec());
    let degrees = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 4326).set("projected", false),
            &two,
            &when(),
        )
        .expect("two objects in degrees");
    assert_eq!(degrees.objects.len(), 2);
}

#[test]
fn crs_writes_an_origin_an_azimuth_and_a_vertical_origin_where_the_schema_puts_them() {
    // A plane turned 30° from grid north,
    // its origin near a platform, a depth axis, and a vertical origin - in the
    // vertical system, its sense and unit (CRS.xsd l. 517-520). To break: an
    // argument dropped on the way to `crs::build`, which would write the
    // identity and read back as a frame nobody asked for.
    let plan = Registry::new()
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new()
                .set("epsg", 2154)
                .set("vertical", 5720)
                .set("originEasting", 651_000.0)
                .set("originNorthing", 6_862_000.0)
                .set("azimuth", 30.0)
                .set("axis", "depth")
                .set("originVertical", 35.0),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let plane = plan.of_role("local-2d-crs").next().expect("the plane");
    assert_eq!(plane.json["OriginProjectedCoordinate1"], json!(651_000.0));
    assert_eq!(plane.json["OriginProjectedCoordinate2"], json!(6_862_000.0));
    assert_eq!(
        plane.json["Azimuth"]["_"],
        json!(30.0),
        "{}",
        plane.json["Azimuth"]
    );
    assert_eq!(plane.json["AzimuthReference"], json!("grid north"));
    let compound = plan.of_role("crs").next().expect("the compound");
    assert_eq!(compound.json["OriginVerticalCoordinate"], json!(35.0));
    assert_eq!(compound.json["VerticalAxis"]["Direction"], json!("down"));
    assert_eq!(compound.json["VerticalAxis"]["IsTime"], json!(false));
}

#[test]
fn two_axes_on_one_code_share_the_halves_and_the_plane_and_differ_by_the_compound() {
    // What the recipe of Lutetia leans on (an origin, an azimuth and a time axis over the base systems):
    // a depth frame and a time frame over Lambert-93 and NGF-IGN69 are **one**
    // projected system, one vertical, one plane - and two compounds. To break:
    // the axis left out of the compound's seed (the second run overwrites the
    // first), or put in the plane's (two planes where there is one).
    let registry = Registry::new();
    let base = Choices::new().set("epsg", 2154).set("vertical", 5720);
    let depth = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &base.clone().set("axis", "depth"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let time = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &base.set("axis", "time"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let uuid =
        |plan: &energyml_build::operation::Plan, role: &str| plan.of_role(role).next().unwrap().uuid.clone();
    for shared in ["projected-crs", "vertical-crs", "local-2d-crs"] {
        assert_eq!(uuid(&depth, shared), uuid(&time, shared), "{shared}");
    }
    assert_ne!(uuid(&depth, "crs"), uuid(&time, "crs"));
    // The time axis counts down in the unit `timeUom` defaults to, `IsTime` set;
    // the vertical system stays a length - a top-level one can be nothing else
    // (CRS.xsd l. 677).
    let axis = &time.of_role("crs").next().unwrap().json["VerticalAxis"];
    assert_eq!(
        (
            axis["Direction"].clone(),
            axis["Uom"].clone(),
            axis["IsTime"].clone()
        ),
        (json!("down"), json!("ms"), json!(true))
    );
    assert_eq!(
        time.of_role("vertical-crs").next().unwrap().json["uom"],
        json!("m")
    );
    // Two runs at one instant are two events, and their activities are two.
    assert_ne!(depth.activity.seed, time.activity.seed);
}

#[test]
fn a_code_of_degrees_under_a_projected_crs_is_refused_and_the_reverse_too() {
    // A defect found by an audit: `crs --set
    // epsg=4326` wrote a `ProjectedCrs` whose numbers every reader takes for
    // metres, and `crs::is_geographic` existed without being called.
    let registry = Registry::new();
    let why = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 4326),
            &Identity::derived(),
            &when(),
        )
        .expect_err("degrees under a ProjectedCrs");
    assert!(
        why.message().contains("4326") && why.message().contains("projected"),
        "{why}"
    );
    let why = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 2154).set("projected", false),
            &Identity::derived(),
            &when(),
        )
        .expect_err("metres under a Geographic2dCrs");
    assert!(why.message().contains("2154"), "{why}");
    // And a local frame over degrees is refused by the builder, naming the
    // schema line that has no room for it.
    let why = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new()
                .set("epsg", 4326)
                .set("projected", false)
                .set("azimuth", 30.0),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no compound over degrees");
    assert!(why.message().contains("504"), "{why}");
}

#[test]
fn a_time_unit_that_is_no_time_is_refused_on_its_slot() {
    // `IsTime` over metres would be a travel time measured with a ruler.
    let why = Registry::new()
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new()
                .set("epsg", 2154)
                .set("axis", "time")
                .set("timeUom", "m"),
            &Identity::derived(),
            &when(),
        )
        .expect_err("a time axis in metres");
    assert!(why.message().contains("timeUom"), "{why}");
}

#[test]
fn crs_applies_into_an_empty_package_with_its_template_and_activity() {
    // No array, so no HDF5 needed: the whole write path in one test. To break:
    // an object that does not read back, a template inserted twice, an
    // activity that names nothing.
    let registry = Registry::new();
    let plan = registry
        .plan(
            "crs",
            &Pieces::empty(),
            &Choices::new().set("epsg", 2154),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    let schemas = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "crs.epc", schemas).expect("a new package");
    let made = apply(&plan, &mut store, &when());
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    // Four objects - the two halves, the plane and the compound, since 24/09 -
    // the template, the activity. The count was four when `crs` wrote two, and
    // it is `apply` that parses each object into its generated type: an
    // embedded `OriginProjectedCrs` or a `VerticalAxis` spelt wrong is refused
    // here, where a JSON assertion would pass over it.
    assert_eq!(plan.objects.len(), 4);
    assert_eq!(made.written.len(), 6, "{:?}", made.written);
    assert!(made.activity.is_some());
    let template_uuid = energyml_domain::provenance::template_uuid("crs").unwrap();
    assert!(store.find_by_uuid(template_uuid).is_some());
    for object in &plan.objects {
        assert!(
            store.find_by_uuid(&object.uuid).is_some(),
            "{} is in the package",
            object.qualified_type
        );
    }

    // Nothing was there: the first writing replaces nothing.
    assert!(made.replaced.is_empty(), "{:?}", made.replaced);

    // Applied again, the template is not inserted twice - and the objects are
    // the same, so the package does not grow a second CRS.
    let again = apply(&plan, &mut store, &when());
    assert!(again.refused.is_empty(), "{:?}", again.refused);
    assert_eq!(again.written.len(), 5, "{:?}", again.written);
    // **And the second says so.** Identity is derived, so replaying replaces
    // its own objects - it is intended, and it is precisely why it is
    // counted: overwriting those of *another* run has the same shape, and nobody
    // would see it otherwise.
    // The four CRS **and the activity**: an activity is new on each run
    // because its uuid derives from the instant, and this test freezes one. That is what
    // makes it useful here - it shows that the count follows the derived identity
    // and nothing else.
    assert_eq!(
        again.replaced.len(),
        5,
        "the four CRS and the activity: {:?}",
        again.replaced
    );
}

/// **The frame a `pointset` names is read back whole, from the geometry.**
///
/// The triangulation that the reference model asks for, and the one that could
/// not pass before: a point set written from a GeoJSON named the `ProjectedCrs`,
/// so its frame read back with a horizontal code and **no vertical one** - the
/// `VerticalCrs` was in the package, referenced by nothing. Two independent
/// sources of one fact: the codes the run was told, and what
/// `energyml_domain::crs::of` reads off the written representation, through the
/// references the package holds.
///
/// To break: `Built::reference` pointing at the `ProjectedCrs` again - the
/// vertical code comes back `None`.
#[test]
fn a_written_point_set_names_a_frame_whose_two_codes_read_back() {
    let collection = geojson::read(
        r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","geometry":{"type":"Point","coordinates":[2.35,48.85,35]},"properties":{}},
            {"type":"Feature","geometry":{"type":"Point","coordinates":[2.36,48.86,40]},"properties":{}}]}"#,
    )
    .unwrap();
    let pieces = Pieces::empty().with(
        "points.geojson",
        Source::Features {
            name: "points.geojson".into(),
            features: collection.features,
        },
    );
    let plan = Registry::new()
        .plan(
            "pointset",
            &pieces,
            &Choices::new()
                .set("project", 2154)
                .set("vertical", 5720)
                .set("title", "Semis"),
            &Identity::derived(),
            &when(),
        )
        .unwrap();
    assert!(plan.blocked.is_none(), "{:?}", plan.blocked);
    let schemas = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "points.epc", schemas).expect("a new package");
    // The objects only: the frame is read without a single array.
    let made = energyml_build::apply::apply_with(&plan, &mut store, &when(), false);
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);

    let representation = plan.of_role("representation").next().expect("the point set");
    let uri = store
        .find_by_uuid(&representation.uuid)
        .expect("in the package")
        .uri
        .clone();
    let object = store.get_object(&uri).expect("it reads");
    let frame = energyml_domain::crs::of(object.as_reflect(), &store)
        .expect("the frame reads")
        .expect("the point set names one");
    assert_eq!(frame.projected.epsg(), Some(2154), "{frame:?}");
    assert_eq!(
        frame.vertical.epsg(),
        Some(5720),
        "the vertical half, reached through the compound: {frame:?}"
    );
    assert!(!frame.is_component(), "a whole system: {frame:?}");
    // The identity: a Lambert-93 easting written under it is one read back.
    assert!(frame.is_identity(), "{frame:?}");
}

/// **An origin, an azimuth and a time axis written by `crs` read back as such** -
/// the two lines of the reference model's steps: *the time frame read back as time;
/// the frame read back with its azimuth of 30°*. The writer and `energyml_domain::crs`'s
/// reader are the two sources; the XSD is the arbiter of what each field means.
#[test]
fn a_turned_and_offset_frame_and_a_time_frame_read_back_as_written() {
    let schemas = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "frames.epc", schemas).expect("a new package");
    let registry = Registry::new();
    let mut compound_of = |choices: Choices| -> String {
        let plan = registry
            .plan("crs", &Pieces::empty(), &choices, &Identity::derived(), &when())
            .unwrap();
        let made = apply(&plan, &mut store, &when());
        assert!(
            made.invalid.is_empty(),
            "objects written against their schema: {:#?}",
            made.invalid
        );
        assert!(made.refused.is_empty(), "{:?}", made.refused);
        plan.of_role("crs").next().unwrap().uuid.clone()
    };
    let turned = compound_of(
        Choices::new()
            .set("epsg", 2154)
            .set("vertical", 5720)
            .set("originEasting", 651_000.0)
            .set("originNorthing", 6_862_000.0)
            .set("azimuth", 30.0)
            .set("axis", "depth")
            .set("originVertical", 35.0),
    );
    let timed = compound_of(
        Choices::new()
            .set("epsg", 2154)
            .set("vertical", 5720)
            .set("axis", "time"),
    );

    let read = |uuid: &str| {
        let uri = store.find_by_uuid(uuid).expect("in the package").uri.clone();
        let object = store.get_object(&uri).expect("it reads");
        energyml_domain::crs::read_with(object.as_reflect(), &store).expect("a frame")
    };
    let frame = read(&turned);
    assert_eq!(
        (frame.x_offset, frame.y_offset),
        (651_000.0, 6_862_000.0),
        "{frame:?}"
    );
    assert_eq!(frame.projected.epsg(), Some(2154));
    assert_eq!(frame.vertical.epsg(), Some(5720));
    // An azimuth of 30° clockwise from grid north is the local Y axis pointing
    // N30E - RESQML 2.0.1 `Common.xsd` l. 136-145 for the sense. So the local
    // point (0, 100, 0) sits 100 m along N30E from the origin, whatever
    // convention the reader keeps internally: the assertion is on the place.
    let [x, y, _] = frame.to_projected([0.0, 100.0, 0.0]);
    let (sin, cos) = 30f64.to_radians().sin_cos();
    assert!((x - (651_000.0 + 100.0 * sin)).abs() < 1e-6, "x = {x}");
    assert!((y - (6_862_000.0 + 100.0 * cos)).abs() < 1e-6, "y = {y}");
    // The axis counts down, and its origin is 35 m **above** the datum - in the
    // vertical system's sense (CRS.xsd l. 517-520): a local depth of 10 is an
    // elevation of 25.
    assert!(!frame.is_time());
    let [_, _, elevation] = frame.to_projected([0.0, 0.0, 10.0]);
    assert!((elevation - 25.0).abs() < 1e-9, "35 − 10 = 25, read {elevation}");

    let time = read(&timed);
    assert!(time.is_time(), "{time:?}");
    assert_eq!(time.vertical_axis_uom(), Some("ms"), "{time:?}");
    assert_eq!(
        time.vertical.epsg(),
        Some(5720),
        "the datum a time is counted from"
    );
}

// -- sigeol, reformulated ---------------------------------------------------

#[test]
fn sigeol_plans_the_same_objects_the_writer_made_and_counts_what_it_dropped() {
    let registry = Registry::new();
    let plan = registry
        .plan(
            "sigeol-units",
            &sigeol_pieces(),
            &Choices::new(),
            &Identity::derived(),
            &when(),
        )
        .expect("planned by suggestion alone");
    let roles: Vec<&str> = plan.objects.iter().map(|o| o.role.as_str()).collect();
    // Two CRS, the table, then feature / interpretation / representation /
    // property per group - two groups: a Bartonien bottom and a Bartonien top.
    assert_eq!(&roles[..3], &["crs", "vertical-crs", "table"]);
    assert_eq!(roles.len(), 3 + 2 * 4, "{roles:?}");
    assert_eq!(plan.datasets.len(), 2);
    // Two features on one place: one point written, one duplicate dropped.
    let total: usize = plan.datasets.iter().map(|d| d.values.len() / 3).sum();
    assert_eq!(total, 2);
    let duplicates = plan
        .activity
        .parameters
        .iter()
        .find_map(|p| match p {
            energyml_domain::provenance::Parameter::Double { title, value, .. }
                if title == "duplicatesDropped" =>
            {
                Some(*value)
            }
            _ => None,
        })
        .expect("counted");
    assert_eq!(duplicates, 1.0);
    // Degrees as they stand: the horizontal system is geographic.
    assert_eq!(plan.objects[0].qualified_type, "eml23.Geographic2dCrs");
}

#[test]
fn sigeol_refuses_given_uuids_because_its_names_belong_to_the_producer() {
    let why = Registry::new()
        .plan(
            "sigeol-units",
            &sigeol_pieces(),
            &Choices::new(),
            &Identity::Given(vec!["11111111-1111-4111-8111-111111111111".into()]),
            &when(),
        )
        .expect_err("refused");
    assert!(why.message().contains("producer"), "{why}");
}

// -- the form is the template -----------------------------------------------

#[test]
fn every_built_in_operation_agrees_with_its_canonical_template() {
    // The rule of `template.rs`: the slot is the source, the file a projection
    // of it. To break: a slot added without its parameter, a kind changed on
    // one side only, a parameter the file declares that no operation does.
    let registry = Registry::new();
    let mut wrong = Vec::new();
    for operation in registry.iter() {
        wrong.extend(template::disagreements(operation));
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_projection_says_what_eml_cannot_type() {
    let registry = Registry::new();
    let projected = template::template_of(registry.get("crs").unwrap());
    let parameters = projected["Parameter"].as_array().unwrap();
    let epsg = parameters.iter().find(|p| p["Title"] == "epsg").unwrap();
    assert_eq!(epsg["AllowedKind"], json!(["double"]));
    assert_eq!(epsg["MinOccurs"], json!(1));
    assert_eq!(epsg["Constraint"], json!("an EPSG code"));
    let projected_flag = parameters.iter().find(|p| p["Title"] == "projected").unwrap();
    // No boolean kind in EML 2.3: a flag is a string that says which two.
    assert_eq!(projected_flag["AllowedKind"], json!(["string"]));
    assert_eq!(projected_flag["Constraint"], json!("true or false"));
    assert_eq!(projected_flag["MinOccurs"], json!(0));
    let output = parameters.last().unwrap();
    assert_eq!(output["Title"], json!("output"));
    assert_eq!(output["IsOutput"], json!(true));
    assert_eq!(output["MaxOccurs"], json!(-1));
}

// -- an inaptitude is not a condition of writing -----------------------------

/// The release vocabulary, as a refusal would spell it.
fn names_a_release(why: &str) -> bool {
    [
        "2.0.1", "2.2", "resqml20", "resqml22", "eml20", "eml23", "release", "version",
    ]
    .iter()
    .any(|word| why.to_lowercase().contains(word))
}

/// **No `Unfit` mentions a release** - the control the specification asks for.
///
/// `Unfit` was carrying refusals of two natures: *this operation has nothing to
/// do with this input* - an `outline` over a number - and *this input could not
/// be written here*. Only the first is an inaptitude; the second is a condition
/// of writing, and it belongs to `Plan::blocked`. An offer marked unfit for the
/// second reason is the defect reported on Volve: the operation computed five
/// boundary rings perfectly well and the front end never let anyone ask for them.
///
/// # What would have to break for it to go red
///
/// A `fitness` that looks at the qualified type of a known object and refuses
/// an older release - which is exactly what `support.rs` did until 11/09, and
/// what a future operation writing 2.2 objects is tempted to do again.
#[test]
fn no_unfitness_names_a_release() {
    // The bag that tempts it: everything a 2.0.1 package hands over - the
    // object, its CRS, and a shape of it.
    let old = Pieces::empty()
        .with(
            "shape",
            Source::Shape {
                geometry: energyml_geometry::geometry::Geometry {
                    kind: "triangles",
                    patches: vec![energyml_geometry::geometry::Patch {
                        index: 0,
                        node_offset: 0,
                        points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                        topology: energyml_geometry::geometry::Topology::Triangles(vec![[0, 1, 2]]),
                        closed: Vec::new(),
                    }],
                },
                closed: None,
            },
        )
        .with(
            "object",
            Source::Object(energyml_build::pieces::Known {
                reference: Some(energyml_build::pieces::Ref::new(
                    "bbbbbbbb-0000-4000-8000-000000000002",
                    "resqml20.obj_TriangulatedSetRepresentation",
                    "A 2.0.1 surface",
                )),
                crs: Some(energyml_build::pieces::Ref::new(
                    "cccccccc-0000-4000-8000-000000000003",
                    "resqml20.obj_LocalDepth3dCrs",
                    "Ancien",
                )),
                vertical_uom: Some("m".into()),
                ..Default::default()
            }),
        );

    let registry = Registry::new();
    let mut said = Vec::new();
    for pieces in [Pieces::empty(), old, sigeol_pieces()] {
        for offer in registry.offers(&pieces) {
            if let Fitness::Unfit(energyml_build::operation::Refusal { message: why, .. }) = offer.fitness {
                assert!(
                    !names_a_release(&why),
                    "`{}` refuses for a reason that belongs to the writing: {why}",
                    offer.operation.name()
                );
                said.push(why);
            }
        }
    }
    // And the control this test needs to prove anything: it did read refusals,
    // and the matcher does catch the thing it looks for. Without both, a
    // `fitness` that became `Fit` for everything would pass in silence.
    assert!(said.len() > 5, "the bags exercised too few refusals: {said:?}");
    assert!(names_a_release("this operation writes RESQML 2.2 objects"));
}

// -- what an operation takes, declared -------------------------------------

/// **The declaration and the verdict must agree.**
///
/// `takes()` says what an operation takes; `fitness()` judges a bag. They are
/// two statements of the same fact, and nothing linked them: an operation could
/// declare *a triangulated shape* and refuse a triangulated shape, or
/// the reverse - a form would then have asked for exactly what the module
/// allait rejeter.
///
/// The check builds, for each operation, **the bag that its own declaration
/// describes**, and requires `fitness` to find it fit.
///
/// # What would have to break to make it turn red
///
/// Declaring a piece kind that `fitness` does not accept, or tightening a
/// `fitness` without touching the declaration. The first case is that of a
/// form that offers a field leading to a refusal; the second, of a field that
/// disappears for no visible reason.
/// The operations whose refusal is about the **content** of a piece, and not
/// about its kind.
///
/// `Takes` says a piece kind and, for a shape, the expected topology -
/// it is a **filter**, which sorts the list of the form; the refusal stays
/// with `fitness`, which alone sees the whole bag. `sigeol-units` does take a
/// collection of features, and requires in addition a GeoSciML vocabulary inside:
/// no kind declaration can say that, and inventing one for it would be
/// a second vocabulary for one operation.
///
/// **A named list rather than a loosened check**: a second entry here
/// asks for the same justification, and writing it is the moment when one asks
/// whether it really is one.
///
/// `sigeol-boreholes` is that second entry, and it is **exactly the same case**,
/// not a neighbouring one: it takes a collection of features and requires a
/// GeoSciML `Borehole` inside. Both read the same format and part ways on
/// the vocabulary that format carries - which a piece kind cannot
/// say, since both take the same kind. Inventing a `borehole-features` kind
/// for it would make `Source::kind` a second vocabulary, the very one
/// that the specification refuses; and writing it here is what keeps the
/// negative check below able to bite.
const CONTENT_REFUSERS: &[&str] = &["sigeol-units", "sigeol-boreholes"];

#[test]
fn what_an_operation_declares_it_takes_makes_it_fit() {
    let registry = Registry::new();
    let mut checked = 0usize;
    for operation in registry.iter() {
        let takes = operation.takes();
        if takes.is_empty() || CONTENT_REFUSERS.contains(&operation.name()) {
            continue;
        }
        let pieces = bag_for(&takes);
        let fitness = operation.fitness(&pieces);
        assert_eq!(
            fitness,
            Fitness::Fit,
            "`{}` declares it takes {:?} and refuses the bag that describes",
            operation.name(),
            takes.iter().map(|s| s.name).collect::<Vec<_>>()
        );
        checked += 1;
    }
    // The negative check: without it, an empty `takes()` everywhere would pass.
    assert!(checked >= 10, "only {checked} operation(s) declare their inputs");

    // And the exception does not widen by itself: what is named must
    // really refuse on content, otherwise the line is dead and the next
    // operation would slip into it unseen.
    for name in CONTENT_REFUSERS {
        let operation = registry
            .get(name)
            .unwrap_or_else(|| panic!("`{name}` is in the registry"));
        let refusal = operation.fitness(&bag_for(&operation.takes()));
        assert!(
            matches!(refusal, Fitness::Unfit(_)),
            "`{name}` is named as refusing on content, and it accepts"
        );
    }
}

/// **`crs` takes nothing, and that is an answer.**
///
/// This is the measurement that motivated the declaration of what is taken: the only operation fit on an
/// empty bag was unreachable from a front end that required an input before
/// proposing anything. An empty `takes()` is what lets the form
/// offer it.
#[test]
fn a_pure_creation_declares_no_input() {
    let registry = Registry::new();
    let crs = registry.get("crs").expect("`crs` is in the registry");
    assert!(crs.takes().is_empty());
    assert_eq!(crs.fitness(&Pieces::empty()), Fitness::Fit);

    // Any other operation fit on nothing must say why here.
    //
    // - `stratigraphy-from-geolts` (24/09): its material is the BRGM chart
    //   that the library embeds (`energyml_domain::geolts`), and what it is
    //   told is **which branches** - a setting, not an input of the
    //   scene;
    // - `business-associate` (25/09): an organisation is named, it is not read
    //   from any piece of the scene.
    let free: Vec<&str> = registry
        .iter()
        .filter(|o| o.takes().is_empty())
        .map(energyml_build::operation::AnyOperation::name)
        .collect();
    assert_eq!(
        free,
        vec!["crs", "stratigraphy-from-geolts", "business-associate"],
        "one more operation takes nothing"
    );
}

/// **An input is not a template parameter.**
///
/// The activity already records its inputs as such; declaring them also
/// in the projection would make the same thing an input *and* a setting, and
/// `template.rs` compares by title - the two would read as a disagreement of
/// direction. That is why `takes()` is separate from `needs()`.
#[test]
fn inputs_do_not_pass_into_the_template() {
    let registry = Registry::new();
    for operation in registry.iter() {
        let taken: Vec<&str> = operation.takes().iter().map(|s| s.name).collect();
        let asked: Vec<&str> = operation.needs(&Pieces::empty()).iter().map(|s| s.name).collect();
        for name in &taken {
            assert!(
                !asked.contains(name),
                "`{}` declares `{name}` as an input **and** as a setting",
                operation.name()
            );
        }
    }
}

/// The bag that this declaration describes - to confront `takes` and `fitness`.
/// An OSDU template entry, for [`bag_for`].
fn column(name: &str, uuid: &str) -> serde_json::Value {
    json!({
        "ColumnName": name,
        "ValueType": "number",
        "PropertyType": { "PropertyTypeID": format!("namespace:reference-data--PropertyType:{uuid}:") },
    })
}

fn bag_for(takes: &[energyml_build::operation::Slot]) -> Pieces {
    use energyml_build::operation::Need;
    use energyml_geometry::geometry::{Geometry, Patch, Topology};

    let hsv = |hue: f64| energyml_domain::color::Hsv {
        hue,
        saturation: 1.0,
        value: 1.0,
        alpha: 1.0,
        title: None,
    };
    let square = || {
        vec![
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
        ]
    };
    let mut pieces = Pieces::empty();
    for slot in takes {
        let Need::Input(wants) = &slot.need else { continue };
        // An optional slot is *one way or the other*: filling both
        // would make a bag that nobody would send. The first is enough.
        if slot.least == 0 && pieces.iter().next().is_some() {
            continue;
        }
        // **As many values as the floor asks for**, named as the front end
        // names them: `surfaces`, `surfaces 2`. One was enough as long as
        // all the operations were `1..1`; the first that asks for
        // two - `cross-section` - made this check red for a reason that
        // was not its own. It is also the first time that the cardinality
        // declared on 11/09 is **exercised** by a check.
        let how_many = slot.least.max(1) as usize;
        let source = match wants.kind {
            "shape" => {
                let (points, topology) = match wants.topology {
                    Some("triangles") => (square(), Topology::Triangles(vec![[0, 1, 2], [0, 2, 3]])),
                    Some("polylines") => (square(), Topology::Polylines(vec![4])),
                    // A 2 × 2 lattice, row by row, the fast axis along x -
                    // `horizon-from-law` (24/09) declares `grid`, and a point
                    // cloud in its place is refused at the offer.
                    Some("grid") => (
                        vec![
                            [0.0, 0.0, 0.0],
                            [10.0, 0.0, 0.0],
                            [0.0, 10.0, 0.0],
                            [10.0, 10.0, 0.0],
                        ],
                        Topology::Grid {
                            fastest: 2,
                            slowest: 2,
                        },
                    ),
                    // One corner-point cell, its eight corners `i` fastest,
                    // then `j`, then the level - `grid-properties` and
                    // `local-grid` (28/09) declare cells, and read them.
                    Some("hexahedra") => (
                        vec![
                            [0.0, 0.0, 0.0],
                            [10.0, 0.0, 0.0],
                            [0.0, 10.0, 0.0],
                            [10.0, 10.0, 0.0],
                            [0.0, 0.0, -10.0],
                            [10.0, 0.0, -10.0],
                            [0.0, 10.0, -10.0],
                            [10.0, 10.0, -10.0],
                        ],
                        Topology::Hexahedra {
                            cells: vec![[0, 1, 3, 2, 4, 5, 7, 6]],
                            dimensions: [1, 1, 1],
                            index: Vec::new(),
                        },
                    ),
                    _ => (square(), Topology::Points),
                };
                Source::Shape {
                    geometry: Geometry {
                        kind: "x",
                        patches: vec![Patch {
                            index: 0,
                            node_offset: 0,
                            points,
                            topology,
                            closed: Vec::new(),
                        }],
                    },
                    closed: None,
                }
            }
            "features" => {
                let text = r#"{"type":"FeatureCollection","features":[
                    {"type":"Feature","geometry":{"type":"LineString","coordinates":[[2.0,48.0,10],[2.1,48.0,11]]},"properties":{}}]}"#;
                Source::Features {
                    name: "essai.geojson".to_string(),
                    features: geojson::read(text).expect("a collection").features,
                }
            }
            // **A plausible table, not an empty one.** The bag that this
            // check builds is the one a caller would send, and for a
            // table that means *with its template*: a table without a mapping
            // means nothing, and the template travels with the file.
            //
            // Not building it here would have exempted `wells-from-table` from the
            // check by filing it under `CONTENT_REFUSERS`, which would have been
            // wrong: what it requires - that the template name the roles - is
            // carried by the piece itself, where a GeoSciML vocabulary is
            // buried in the features.
            "table" => {
                let text = "SONDAGE,X,Y,Z_MNT,PROF
F1,650000,6800000,120,0
F1,650000,6800000,120,25
";
                let table =
                    energyml_build::tabular::read("essai.csv", text, Default::default()).expect("a table");
                let template = energyml_build::osdu::from_json(&json!([
                    column("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1"),
                    column("X", "620cec71-5ce3-4ea5-9120-833756b427d7"),
                    column("Y", "cfcfdb04-02c1-4262-b66b-459b0aab3e8e"),
                    column("Z_MNT", "8508cef4-0ade-454a-9ea4-e00d5905e677"),
                    column("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5"),
                ]))
                .expect("a template");
                Source::Table {
                    name: "essai.csv".to_string(),
                    table,
                    template,
                }
            }
            "lattice" => Source::Lattice(energyml_build::pieces::Lattice {
                origin: [0.0, 0.0],
                spacing: [1.0, 1.0],
                counts: [2, 2],
                z: vec![0.0, 1.0, 2.0, 3.0],
            }),
            "field" => Source::Field {
                over: "shape".to_string(),
                on: "nodes".to_string(),
                values: vec![1.0, 2.0, 3.0, 4.0],
                uom: None,
            },
            "ramp" => Source::Ramp {
                // One colour per band, and a band starts at each level:
                // three levels make two bands.
                levels: vec![0.0, 10.0, 20.0],
                colours: vec![hsv(0.0), hsv(120.0), hsv(240.0)],
            },
            // **An object of the package, of the first type that the slot declares** -
            // or of any type if the slot takes them all. `collection`
            // (24/09) is the first operation whose object input is
            // required without `every`: without this case, the bag stayed empty and the
            // check went red for a reason that was not its own.
            "object" => Source::Object(energyml_build::pieces::Known {
                reference: Some(energyml_build::pieces::Ref {
                    uuid: "5d2c6fa7-8192-43a4-bfe0-5b6c7d8e9fa0".into(),
                    qualified_type: wants
                        .types
                        .first()
                        .copied()
                        .unwrap_or("resqml22.Model")
                        .to_string(),
                    title: slot.name.to_string(),
                    object_version: None,
                }),
                ..Default::default()
            }),
            // A file carried as it is - `media` (25/09), the only operation
            // that takes one.
            "document" => Source::Document {
                name: "README.md".to_string(),
                content_type: Some("text/markdown".to_string()),
                bytes: b"# a file\n".to_vec(),
            },
            _ => continue,
        };
        // **A shape whose slot names types is an object of the package**,
        // and it arrives with its companion `X (object)` - what
        // `read::in_slot` writes. `export-media` (25/09) is the first to
        // refuse without it: a file linked to what it shows needs a uuid, and
        // an artefact has none. Without this case, the check went red for a
        // reason that was not its own.
        let companion = (wants.kind == "shape" && !wants.types.is_empty()).then(|| {
            Source::Object(energyml_build::pieces::Known {
                reference: Some(energyml_build::pieces::Ref {
                    uuid: "8e4b2a1c-3d5f-4e6a-9b7c-0d1e2f3a4b5c".into(),
                    qualified_type: wants.types[0].to_string(),
                    title: slot.name.to_string(),
                    object_version: None,
                }),
                ..Default::default()
            })
        });
        for rank in 0..how_many {
            let name = if rank == 0 {
                slot.name.to_string()
            } else {
                format!("{} {}", slot.name, rank + 1)
            };
            pieces = pieces.with(name.clone(), source.clone());
            if let Some(companion) = &companion {
                pieces = pieces.with(format!("{name} (object)"), companion.clone());
            }
        }
    }
    pieces
}

/// What a slot **declares it creates** must be what the plan writes.
///
/// `Otherwise::Creates` exists so that an empty reads as *I make one*
/// rather than as *you may do without*, and to **name** what will be
/// made. A declaration that names something else is worse than an absence: the
/// form promises an object that the package will never have, and nobody will
/// check it.
///
/// Nobody had checked it. The `sourceEpsg` slot announced an
/// `eml23.LocalEngineeringCompoundCrs` that `crs::built` did not write - seen
/// on 21/09 by **counting the types written** by an import of 936 boreholes: a
/// `ProjectedCrs`, a `VerticalCrs`, no compound. Since 24/09 the
/// compound is written (RESQML TRG 2.2 §3.1 p. 22), it came back into the
/// declaration, and this check requires both directions: nothing written that is not
/// declared, nothing declared that no path writes.
///
/// The confrontation is therefore against a **plan**, never against `makes()` - which
/// only lists what the operation produces by itself, not what its frame
/// makes. That was the first attempt, and it went red for the wrong reason.
#[test]
fn what_a_slot_declares_it_creates_is_what_the_plan_writes() {
    let registry = Registry::new();
    let operation = registry.get("pointset").expect("pointset");

    // The slot declares both forms because the answer depends on
    // `project`, which a static declaration cannot read. Each plan must
    // therefore write **one of the two**, and the vertical in both cases.
    let declared: Vec<&str> = operation
        .needs(&Pieces::empty())
        .into_iter()
        .find(|slot| slot.name == "sourceEpsg")
        .map(|slot| match slot.otherwise {
            Otherwise::Creates { types } => types,
            _ => panic!("`sourceEpsg` must declare what its empty creates"),
        })
        .expect("the sourceEpsg slot");

    let cloud = Source::Shape {
        geometry: energyml_geometry::Geometry {
            kind: "cloud",
            patches: vec![energyml_geometry::Patch {
                index: 0,
                node_offset: 0,
                points: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [2.0, 0.0, 0.0]],
                topology: energyml_geometry::Topology::Points,
                closed: Vec::new(),
            }],
        },
        closed: None,
    };
    // **Three paths since 24/09**, and each writes what it should: a
    // system in metres, projected or taken as it stands (without `project`, the target is
    // the source), writes the compound; degrees write their
    // `Geographic2dCrs`, the named exception. Together they must write
    // **everything** that the slot declares - the converse of the contract, held in full
    // rather than by an "at least one horizontal".
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (source, project) in [(2154, Some(2154)), (2154, None), (4326, None)] {
        let mut choices = Choices::new()
            .set("sourceEpsg", source)
            .set("title", "Nuage")
            .set("dataFile", "data.h5");
        if let Some(code) = project {
            choices = choices.set("project", code);
        }
        let plan = registry
            .plan(
                "pointset",
                &Pieces::empty().with("shape", cloud.clone()),
                &choices,
                // Enough to name each object of the plan: the frame, its vertical
                // and the representation. Wider than necessary, because this
                // test is about what is **written**, not about the uuid count.
                &Identity::Given(
                    (0..12)
                        .map(|n| format!("00000000-0000-4000-8000-{n:012}"))
                        .collect(),
                ),
                &When {
                    creation: "2026-09-21T00:00:00Z".to_string(),
                    originator: "tests".to_string(),
                },
            )
            .expect("un plan");

        let written: Vec<String> = plan
            .objects
            .iter()
            .map(|one| one.qualified_type.clone())
            .collect();
        for made in &written {
            if !made.contains("Crs") {
                continue;
            }
            assert!(
                declared.contains(&made.as_str()),
                "the plan writes `{made}`, which the slot does not declare: {declared:?}",
            );
        }
        // And the converse of the same contract: what is declared must arrive. At
        // least one horizontal and the vertical, otherwise the declaration promises
        // objects that no path produces.
        assert!(
            written.iter().any(|one| one == "eml23.VerticalCrs"),
            "the vertical is declared and not written: {written:?}",
        );
        assert!(
            written
                .iter()
                .any(|one| one == "eml23.ProjectedCrs" || one == "eml23.Geographic2dCrs"),
            "no horizontal written: {written:?}",
        );
        // And what the geometry names is the object of role `crs`: the compound
        // in metres, the `Geographic2dCrs` in degrees.
        let frame = plan.of_role("crs").next().expect("the frame");
        let expected = if source == 4326 {
            "eml23.Geographic2dCrs"
        } else {
            "eml23.LocalEngineeringCompoundCrs"
        };
        assert_eq!(
            frame.qualified_type, expected,
            "source {source}, project {project:?}"
        );
        let representation = plan.of_role("representation").next().expect("the cloud");
        assert_eq!(
            representation.json["NodePatchGeometry"][0]["LocalCrs"]["Uuid"],
            json!(frame.uuid)
        );
        seen.extend(written.into_iter().filter(|one| one.contains("Crs")));
    }
    for one in &declared {
        assert!(
            seen.contains(*one),
            "the slot declares `{one}` and no path writes it: {seen:?}"
        );
    }
}
