//! `section-view` - the line a section was drawn along, written as a
//! section-line feature with its polylines, and the drawing attached to it.
//!
//! The form of the assertions comes from what the inputs **state**, never from
//! what the code computes:
//!
//! - the long section's trace is the coordinates this test typed, node for
//!   node;
//! - `Crosses::Count` spreads its cross sections evenly, **ends included**
//!   (`energyml_section::model::Crosses`), each running from its `before`
//!   end on the left of the trace to its `after` end on the right - so on a
//!   trace heading east, three cross sections stand at its two ends and its
//!   middle, north to south, `before` long northward and `after` southward;
//! - an `eml23.Attachment` of the part-address form names its part in
//!   `ContentUri` and the object in `ObjectReference` (`Attachment.xsd`), and
//!   the part is related to the `Attachment` (EPC v1.0 §3.4.2 pp. 15-16) -
//!   checked by **the reader of the domain**, `energyml_domain::attachments`,
//!   not by looking at what the plan said;
//! - a PNG starts with its signature and its `IHDR` (PNG §5.2, §5.6).
//!
//! What would make them fail: a trace taken from anywhere but the drawing, the
//! cross sections dropped or computed another way, the images attached to the
//! feature or to nothing, a PNG of another drawing accepted, a system that
//! disagrees with the drawing's taken without a word.

use std::sync::Arc;

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::attachments::{self, Form, Location};
use energyml_domain::package_index::PackageIndex;
use energyml_section::model::{CrossWidths, Crosses, Inputs, SectionKey, System, Target, View};
use serde_json::{Value, json};

const EAST: f64 = 651_000.0;
const NORTH: f64 = 6_861_000.0;

fn when() -> When {
    When {
        creation: "2026-10-03T00:00:00Z".to_string(),
        originator: "tests/section_view.rs".to_string(),
    }
}

fn inputs() -> Inputs {
    Inputs {
        trace: vec![[EAST, NORTH], [EAST + 1000.0, NORTH]],
        system: Some(System {
            epsg: 2154,
            unit: "m".to_string(),
        }),
        left: 100.0,
        right: 100.0,
        crosses: Crosses::Count {
            count: 3,
            common: CrossWidths {
                before: 150.0,
                after: 50.0,
                left: 20.0,
                right: 20.0,
            },
            own: Vec::new(),
        },
        objects: Vec::new(),
        time_factor: None,
    }
}

/// The section drawn by the section view itself - its metadata included.
fn drawing(inputs: &Inputs) -> String {
    let catalogue = energyml_section::compute(inputs, |_, _| unreachable!("nothing is asked"))
        .expect("the inputs are valid");
    energyml_section::draw(
        &catalogue,
        &View::new(
            Target::Section {
                key: SectionKey::Long,
            },
            [800.0, 400.0],
        ),
    )
}

/// The first 24 bytes of a PNG of `w` × `h` - all the operation reads of one.
fn png(w: u32, h: u32) -> Vec<u8> {
    let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
    bytes.extend_from_slice(b"IHDR");
    bytes.extend_from_slice(&w.to_be_bytes());
    bytes.extend_from_slice(&h.to_be_bytes());
    bytes.extend_from_slice(b"--not a real image--");
    bytes
}

fn bag(svg: &str, raster: Option<Vec<u8>>) -> Pieces {
    let mut pieces = Pieces::empty().with(
        "svg",
        Source::Document {
            name: "long.svg".to_string(),
            content_type: None,
            bytes: svg.as_bytes().to_vec(),
        },
    );
    if let Some(bytes) = raster {
        pieces = pieces.with(
            "png",
            Source::Document {
                name: "long.png".to_string(),
                content_type: None,
                bytes,
            },
        );
    }
    pieces
}

fn choices() -> Choices {
    Choices::new()
        .set("sourceEpsg", 2154)
        .set("project", 2154)
        .set("vertical", 5720)
        .set("elevation", 35.0)
        .set("title", "Section A-A'")
}

fn plan(pieces: &Pieces, choices: Choices) -> Result<Plan, Vec<(Option<String>, String)>> {
    Registry::new()
        .plan("section-view", pieces, &choices, &Identity::derived(), &when())
        .map_err(|error| {
            error
                .refusals()
                .iter()
                .map(|one| (one.slot.clone(), one.message.clone()))
                .collect()
        })
}

fn said(refusals: &[(Option<String>, String)], slot: &str) -> String {
    refusals
        .iter()
        .find(|(on, _)| on.as_deref() == Some(slot))
        .map(|(_, why)| why.clone())
        .unwrap_or_else(|| panic!("nothing refused on `{slot}`: {refusals:?}"))
}

fn object<'a>(plan: &'a Plan, qualified: &str) -> Vec<&'a Value> {
    plan.objects
        .iter()
        .filter(|o| o.qualified_type == qualified)
        .map(|o| &o.json)
        .collect()
}

/// The nodes of each patch, as the plan writes them into the external file.
fn patches(plan: &Plan) -> Vec<Vec<[f64; 3]>> {
    plan.datasets
        .iter()
        .map(|d| match &d.values {
            energyml_build::operation::Numbers::F64(v) => v.chunks(3).map(|c| [c[0], c[1], c[2]]).collect(),
            other => panic!("points are doubles, got {other:?}"),
        })
        .collect()
}

#[test]
fn the_lines_written_are_the_lines_the_drawing_states() {
    let plan =
        plan(&bag(&drawing(&inputs()), Some(png(1200, 600))), choices()).unwrap_or_else(|e| panic!("{e:?}"));

    // The feature is a section line, interpreted, and the lines represent it.
    let [feature] = object(&plan, "resqml22.CulturalFeature")[..] else {
        panic!("one feature")
    };
    assert_eq!(feature["CulturalFeatureKind"], "energyml-rust:section line");
    let [interpretation] = object(&plan, "resqml22.GenericFeatureInterpretation")[..] else {
        panic!("one interpretation")
    };
    assert_eq!(interpretation["InterpretedFeature"]["Uuid"], feature["uuid"]);
    let [lines] = object(&plan, "resqml22.PolylineSetRepresentation")[..] else {
        panic!("one polyline set")
    };
    assert_eq!(lines["RepresentedObject"]["Uuid"], interpretation["uuid"]);

    // Patch 0: the trace this test typed, at the elevation asked.
    let nodes = patches(&plan);
    assert_eq!(nodes.len(), 2, "the long section and its cross sections");
    assert_eq!(nodes[0], vec![[EAST, NORTH, 35.0], [EAST + 1000.0, NORTH, 35.0]]);
    // Patch 1: three cross sections, at the ends and the middle, north to south.
    let runs = lines["LinePatch"][1]["NodeCountPerPolyline"]["Values"]
        .as_array()
        .unwrap();
    assert_eq!(runs.len(), 3);
    let mut at = 0usize;
    for (k, run) in runs.iter().enumerate() {
        let run = run.as_u64().unwrap() as usize;
        let (first, last) = (nodes[1][at], nodes[1][at + run - 1]);
        let x = EAST + 500.0 * k as f64;
        for (got, want) in [(first, [x, NORTH + 150.0, 35.0]), (last, [x, NORTH - 50.0, 35.0])] {
            assert!(
                got.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6),
                "cross section {k}: {got:?}, expected {want:?}"
            );
        }
        at += run;
    }
    assert_eq!(lines["LinePatch"][1]["ClosedPolylines"]["Value"], json!(false));

    // Every setting of the run is a parameter of its own - the values this
    // test typed in `inputs()` and in the view, never a JSON string.
    assert_eq!(plan.activity.recipe, "section-view");
    let recorded = recorded(&plan);
    for (title, value) in [
        ("left", json!(100.0)),
        ("right", json!(100.0)),
        ("crosses", json!("count")),
        ("crossCount", json!(3.0)),
        ("crossBefore", json!(150.0)),
        ("crossAfter", json!(50.0)),
        ("crossLeft", json!(20.0)),
        ("crossRight", json!(20.0)),
        ("draw", json!("long")),
        ("width", json!(800.0)),
        ("height", json!(400.0)),
        ("labels", json!("full")),
    ] {
        assert_eq!(
            recorded
                .iter()
                .filter(|(t, _)| t == title)
                .map(|(_, v)| v.clone())
                .collect::<Vec<_>>(),
            vec![value],
            "`{title}`"
        );
    }
    // A list is one parameter per entry: what `View::new` shows by default.
    let shown: Vec<Value> = recorded
        .iter()
        .filter(|(t, _)| t == "shown")
        .map(|(_, v)| v.clone())
        .collect();
    assert!(shown.contains(&json!("names")) && shown.len() > 1, "{shown:?}");
    // And no parameter is longer than a `StringParameter` holds (`Activities.xsd`).
    assert!(
        recorded
            .iter()
            .all(|(_, v)| v.as_str().is_none_or(|s| s.chars().count() <= 2000))
    );
}

/// Every parameter of the activity, as (title, value) - a reference as its uuid.
fn recorded(plan: &Plan) -> Vec<(String, Value)> {
    use energyml_domain::provenance::Parameter;
    plan.activity
        .parameters
        .iter()
        .map(|p| match p {
            Parameter::Double { title, value, .. } => (title.clone(), json!(value)),
            Parameter::Text { title, value } => (title.clone(), json!(value)),
            Parameter::Object { title, reference } => (title.clone(), reference["Uuid"].clone()),
        })
        .collect()
}

#[test]
fn a_setting_that_disagrees_with_the_drawing_is_refused_on_its_slot() {
    let svg = drawing(&inputs());
    let why = said(
        &plan(&bag(&svg, None), choices().set("left", 250.0)).expect_err("accepted"),
        "left",
    );
    assert!(why.contains("250") && why.contains("100"), "{why}");
    let why = said(
        &plan(&bag(&svg, None), choices().set("timeFactor", 2.0)).expect_err("accepted"),
        "timeFactor",
    );
    assert!(why.contains("says nothing"), "{why}");
    // The drawing's own value, said again, is the drawing.
    plan(&bag(&svg, None), choices().set("left", 100).set("crossCount", 3))
        .unwrap_or_else(|e| panic!("{e:?}"));
}

/// The objects the section was computed from are the activity's inputs, by
/// reference with the package's titles - and one the package does not hold is
/// refused by its uri: the drawing was made from another package.
#[test]
fn the_objects_the_drawing_was_computed_from_are_its_inputs() {
    const A: &str = "0aadacd8-e3a9-54af-aeef-06baa6b28526";
    const B: &str = "10f0e1a4-46a1-5e07-86fc-3d22b0ac3d8c";
    let mut asked = inputs();
    asked.objects = vec![energyml_section::model::Asked {
        store: "s1".to_string(),
        revision: 0,
        wells: vec![format!("eml:///resqml22.WellboreFrameRepresentation({A})")],
        surfaces: vec![format!("eml:///resqml22.Grid2dRepresentation({B})")],
    }];
    let catalogue =
        energyml_section::compute(&asked, |_, _| Default::default()).expect("the inputs are valid");
    let svg = energyml_section::draw(
        &catalogue,
        &View::new(
            Target::Section {
                key: SectionKey::Long,
            },
            [800.0, 400.0],
        ),
    );
    let held = |uuid: &str, kind: &str, title: &str| {
        Source::Object(Known {
            reference: Some(Ref::new(uuid, kind, title)),
            ..Default::default()
        })
    };
    let pieces = bag(&svg, None)
        .with(
            "objects",
            held(A, "resqml22.WellboreFrameRepresentation", "GTH-1 - frame"),
        )
        .with(
            "objects 2",
            held(B, "resqml22.Grid2dRepresentation", "H3 - top of the Ypresian"),
        )
        .with(
            "objects 3",
            held(
                "5f2c0000-0000-4000-8000-000000000000",
                "resqml22.Grid2dRepresentation",
                "not asked",
            ),
        );
    let plan = plan(&pieces, choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let references: Vec<Value> = plan
        .activity
        .parameters
        .iter()
        .filter_map(|p| match p {
            energyml_domain::provenance::Parameter::Object { title, reference } if title == "input" => {
                Some(reference.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        references.len(),
        2,
        "the two asked, not the third: {references:?}"
    );
    assert_eq!(references[0]["Title"], "GTH-1 - frame");
    assert_eq!(references[1]["Uuid"], B);

    let partial = bag(&svg, None).with(
        "objects",
        held(A, "resqml22.WellboreFrameRepresentation", "GTH-1 - frame"),
    );
    let why = said(&plan_of(&partial), "objects");
    assert!(why.contains(B), "{why}");
}

/// A drawing over several packages names objects this package does not hold:
/// they are said, not refused - and only those of the package named by
/// `store` are its inputs.
#[test]
fn another_packages_objects_are_said_and_not_asked_of_this_one() {
    const A: &str = "0aadacd8-e3a9-54af-aeef-06baa6b28526";
    const B: &str = "10f0e1a4-46a1-5e07-86fc-3d22b0ac3d8c";
    let mut asked = inputs();
    asked.objects = vec![
        energyml_section::model::Asked {
            store: "s1".to_string(),
            revision: 0,
            wells: vec![format!("eml:///resqml22.WellboreFrameRepresentation({A})")],
            surfaces: Vec::new(),
        },
        energyml_section::model::Asked {
            store: "s2".to_string(),
            revision: 0,
            wells: Vec::new(),
            surfaces: vec![format!("eml:///resqml22.Grid2dRepresentation({B})")],
        },
    ];
    let catalogue =
        energyml_section::compute(&asked, |_, _| Default::default()).expect("the inputs are valid");
    let svg = energyml_section::draw(
        &catalogue,
        &View::new(
            Target::Section {
                key: SectionKey::Long,
            },
            [800.0, 400.0],
        ),
    );
    let first = bag(&svg, None).with(
        "objects",
        Source::Object(Known {
            reference: Some(Ref::new(
                A,
                "resqml22.WellboreFrameRepresentation",
                "GTH-1 - frame",
            )),
            ..Default::default()
        }),
    );

    let plan = plan(&first, choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let inputs: Vec<&Value> = plan
        .activity
        .parameters
        .iter()
        .filter_map(|p| match p {
            energyml_domain::provenance::Parameter::Object { title, reference } if title == "input" => {
                Some(reference)
            }
            _ => None,
        })
        .collect();
    assert_eq!(inputs.len(), 1, "{inputs:?}");
    assert_eq!(inputs[0]["Uuid"], A);
    let note = plan
        .says
        .iter()
        .find(|n| n.key == "other-packages")
        .expect("the other package is said");
    assert!(note.detail.starts_with("1 object"), "{}", note.detail);

    // Stored in the second package, it is the second's objects that are asked.
    let why = said(&plan_of_with(&first, choices().set("store", "s2")), "objects");
    assert!(why.contains(B) && !why.contains(A), "{why}");
}

/// Storing a section again - redrawn, so other bytes - writes its drawings
/// over themselves: the same `Attachment`s under the same parts, never a
/// second pair beside the first.
#[test]
fn a_section_stored_again_replaces_its_drawings() {
    let once =
        plan(&bag(&drawing(&inputs()), Some(png(1200, 600))), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let wider = energyml_section::draw(
        &energyml_section::compute(&inputs(), |_, _| unreachable!("nothing is asked"))
            .expect("the inputs are valid"),
        &View::new(
            Target::Section {
                key: SectionKey::Long,
            },
            [1000.0, 500.0],
        ),
    );
    let again = plan(&bag(&wider, Some(png(1000, 500))), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    assert_ne!(once.media[0].bytes, again.media[0].bytes, "the test redraws");

    let attachments = |plan: &Plan| {
        let mut found: Vec<(String, String)> = plan
            .objects
            .iter()
            .filter(|o| o.qualified_type == "eml23.Attachment")
            .map(|o| {
                (
                    o.uuid.clone(),
                    o.json["ContentUri"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect();
        found.sort();
        found
    };
    assert_eq!(attachments(&once).len(), 2);
    assert_eq!(attachments(&once), attachments(&again));
    // Each part is related to the attachment that names it.
    for part in &again.media {
        let named: Vec<&str> = again
            .objects
            .iter()
            .filter(|o| o.json["ContentUri"] == part.path.as_str())
            .map(|o| o.uuid.as_str())
            .collect();
        assert_eq!(part.related_to, named, "{}", part.path);
    }

    // Applied one after the other, the lines still carry two drawings.
    let lines = once
        .objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.PolylineSetRepresentation")
        .unwrap()
        .uuid
        .clone();
    let mut store = energyml_build::epc::open(None, "section.epc", Arc::new(energyml_schemas::all()))
        .expect("an empty package");
    for plan in [&once, &again] {
        let made = energyml_build::apply::apply_with(plan, &mut store, &when(), false);
        assert!(
            made.invalid.is_empty(),
            "objects written against their schema: {:#?}",
            made.invalid
        );
        assert!(made.refused.is_empty(), "{:?}", made.refused);
    }
    let index = PackageIndex::build(&store, store.revision());
    let listing = attachments::attachments_of(&store, &index, &[lines.as_str()]).remove(0);
    assert_eq!(listing.pieces.len(), 2, "{:?}", listing.pieces);
}

fn plan_of_with(pieces: &Pieces, choices: Choices) -> Vec<(Option<String>, String)> {
    plan(pieces, choices).expect_err("accepted")
}

fn plan_of(pieces: &Pieces) -> Vec<(Option<String>, String)> {
    plan(pieces, choices()).expect_err("accepted")
}

#[test]
fn the_images_are_parts_attached_to_the_lines_and_results_of_the_activity() {
    let plan =
        plan(&bag(&drawing(&inputs()), Some(png(1200, 600))), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let lines = plan
        .objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.PolylineSetRepresentation")
        .unwrap();

    let mut store = energyml_build::epc::open(None, "section.epc", Arc::new(energyml_schemas::all()))
        .expect("an empty package");
    let made = energyml_build::apply::apply_with(&plan, &mut store, &when(), false);
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);

    // What the reader of the domain finds on the lines.
    let index = PackageIndex::build(&store, store.revision());
    let listing = attachments::attachments_of(&store, &index, &[lines.uuid.as_str()]).remove(0);
    let mut found: Vec<(Location, Option<String>, Option<String>)> = listing
        .pieces
        .iter()
        .inspect(|piece| assert_eq!(piece.form, Form::PartAddress, "{piece:?}"))
        // The lines' own - attached to the feature, they would be inherited
        // through the interpretation, and listed all the same.
        .inspect(|piece| {
            assert_eq!(
                (piece.provenance.kind, piece.provenance.carrier.as_str()),
                ("self", lines.uuid.as_str()),
                "{piece:?}"
            )
        })
        .map(|piece| {
            (
                piece.location.clone(),
                piece.media_type.clone(),
                piece.category.clone(),
            )
        })
        .collect();
    found.sort_by(|a, b| format!("{:?}", a.0).cmp(&format!("{:?}", b.0)));
    assert_eq!(
        found,
        vec![
            // A folder named after the section - `Section A-A'`, what a part
            // name may hold of it.
            (
                Location::Part("media/sections/Section_A-A/long.png".into()),
                Some("image/png".into()),
                Some("SectionView".into())
            ),
            (
                Location::Part("media/sections/Section_A-A/long.svg".into()),
                Some("image/svg+xml".into()),
                Some("SectionView".into())
            ),
        ]
    );

    // The activity names each attachment among its outputs.
    let activity = made.activity.as_ref().expect("an activity is written");
    let read = energyml_core::json::write_value(
        store
            .get_object(activity)
            .expect("the activity reads")
            .as_reflect(),
    );
    let outputs: Vec<&str> = read["Parameter"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["Title"] == "output")
        .filter_map(|p| p["DataObject"]["Uuid"].as_str())
        .collect();
    for attachment in plan
        .objects
        .iter()
        .filter(|o| o.qualified_type == "eml23.Attachment")
    {
        assert!(
            outputs.contains(&attachment.uuid.as_str()),
            "{} is not an output: {outputs:?}",
            attachment.title
        );
    }
}

#[test]
fn a_system_the_drawing_does_not_state_is_refused_with_both() {
    let refusals = plan(
        &bag(&drawing(&inputs()), None),
        choices().set("sourceEpsg", 27572).set("project", 2154),
    )
    .expect_err("accepted");
    let why = said(&refusals, "sourceEpsg");
    assert!(why.contains("EPSG:2154") && why.contains("EPSG:27572"), "{why}");
}

#[test]
fn a_png_of_another_drawing_or_no_png_at_all_is_refused() {
    let svg = drawing(&inputs());
    // 800×400 drawn; a square is another drawing.
    let refusals = plan(&bag(&svg, Some(png(800, 800))), choices()).expect_err("accepted");
    assert!(said(&refusals, "png").contains("800×800"));
    // A JPEG named `.png` is not a PNG.
    let refusals =
        plan(&bag(&svg, Some(b"\xff\xd8\xff\xe0 a jpeg".to_vec())), choices()).expect_err("accepted");
    assert!(said(&refusals, "png").contains("not a PNG"));
    // One pixel of rounding is the same drawing.
    plan(&bag(&svg, Some(png(1201, 600))), choices()).unwrap_or_else(|e| panic!("{e:?}"));
}

#[test]
fn an_image_that_is_not_a_section_view_is_refused_by_name() {
    let refusals = plan(
        &bag("<svg xmlns=\"http://www.w3.org/2000/svg\"/>", None),
        choices(),
    )
    .expect_err("accepted");
    assert!(said(&refusals, "svg").contains("no section-view metadata"));
    let refusals = plan(
        &bag(&drawing(&inputs()), None),
        choices().set("elevation", f64::NAN),
    )
    .expect_err("accepted");
    assert!(!refusals.is_empty());
}

/// A local frame as a package states it: identity unless told otherwise.
fn frame(uuid: &str, kind: &str, down: bool) -> Known {
    use energyml_domain::crs::{Crs, CrsKind, VerticalDirection, VerticalQuantity};
    let placement = Crs {
        uuid: uuid.to_string(),
        kind: "LocalEngineeringCompoundCrs",
        x_offset: 0.0,
        y_offset: 0.0,
        z_offset: 0.0,
        areal_rotation: 0.0,
        vertical_direction: if down {
            VerticalDirection::Down
        } else {
            VerticalDirection::Up
        },
        vertical_quantity: VerticalQuantity::Length,
        horizontal_uom: Some("m".into()),
        vertical_uom: Some("m".into()),
        time_uom: None,
        projected: CrsKind::Epsg(2154),
        vertical: CrsKind::Epsg(5720),
    };
    let reference = Ref::new(uuid, kind, format!("frame {uuid}"));
    Known {
        reference: Some(reference.clone()),
        crs: Some(reference),
        crs_epsg: Some(2154),
        vertical_epsg: Some(5720),
        vertical_up: Some(!down),
        placement: Some(placement),
        ..Default::default()
    }
}

/// The need: before saving, check whether a compatible projected/vertical
/// and compoundCrs already exist in the target package. Compatible
/// is what would be created, the sense aside: a compound of that projected code,
/// no offset, no rotation, a length in metres - one counting up first.
#[test]
fn a_frame_of_the_package_is_compatible_when_it_is_what_would_be_created() {
    const COMPOUND: &str = "eml23.LocalEngineeringCompoundCrs";
    let up = frame("b-up", COMPOUND, false);
    let down = frame("a-down", COMPOUND, true);
    let mut moved = frame("c-moved", COMPOUND, false);
    moved.placement.as_mut().unwrap().x_offset = 651_600.0;
    let mut turned = frame("d-turned", COMPOUND, false);
    turned.placement.as_mut().unwrap().areal_rotation = 0.5;
    let mut timed = frame("e-time", COMPOUND, true);
    timed.placement.as_mut().unwrap().vertical_quantity = energyml_domain::crs::VerticalQuantity::Time;
    let mut feet = frame("f-feet", COMPOUND, false);
    feet.placement.as_mut().unwrap().vertical_uom = Some("ft".into());
    let projected = frame("g-projected", "eml23.ProjectedCrs", false);
    let mut other = frame("h-other", COMPOUND, false);
    other.crs_epsg = Some(27572);

    let all = [
        down.clone(),
        moved,
        turned,
        timed,
        feet,
        projected,
        other,
        up.clone(),
    ];
    let found: Vec<&str> = energyml_build::crs::compatible_frames(&all, 2154)
        .iter()
        .map(|k| k.reference.as_ref().unwrap().uuid.as_str())
        .collect();
    assert_eq!(found, vec!["b-up", "a-down"]);
    assert!(energyml_build::crs::compatible_frames(&all, 32631).is_empty());
}

/// Stored in a frame of the package counting down, the elevation is written as
/// its depth - the lines are at 35 m above the datum either way.
#[test]
fn under_a_frame_counting_down_the_elevation_is_written_as_its_depth() {
    let depth = frame(
        "0b4f0000-0000-4000-8000-000000000001",
        "eml23.LocalEngineeringCompoundCrs",
        true,
    );
    let dor = depth.crs.as_ref().unwrap().dor();
    let choices = Choices::new()
        .set("crs", dor)
        .set("elevation", 35.0)
        .set("title", "Section A-A'");
    let pieces = bag(&drawing(&inputs()), None).with("crs", Source::Object(depth));
    let plan = plan(&pieces, choices).unwrap_or_else(|e| panic!("{e:?}"));
    let nodes = patches(&plan);
    assert_eq!(
        nodes[0],
        vec![[EAST, NORTH, -35.0], [EAST + 1000.0, NORTH, -35.0]]
    );
    // And no frame is created: the plan writes none of the four.
    assert!(
        plan.objects.iter().all(|o| !o.qualified_type.ends_with("Crs")),
        "{:?}",
        plan.objects.iter().map(|o| &o.qualified_type).collect::<Vec<_>>()
    );
}

// -- further drawings -------------------------------------------------------

/// The need: an attachment as a folder that holds all the exports.
/// A section is exported as several files - its cross sections, its plan view -
/// and each is attached to the lines the way the first is.
fn view_of(inputs: &Inputs, target: Target) -> String {
    let catalogue = energyml_section::compute(inputs, |_, _| unreachable!("nothing is asked"))
        .expect("the inputs are valid");
    energyml_section::draw(&catalogue, &View::new(target, [800.0, 400.0]))
}

/// The long section and its PNG, then further files under the slot names the
/// front end uses: `drawings`, `drawings 2`…
fn bag_with_drawings(files: &[(&str, Vec<u8>)]) -> Pieces {
    let mut pieces = bag(&drawing(&inputs()), Some(png(1200, 600)));
    for (rank, (name, bytes)) in files.iter().enumerate() {
        let slot = if rank == 0 {
            "drawings".to_string()
        } else {
            format!("drawings {}", rank + 1)
        };
        pieces = pieces.with(
            &slot,
            Source::Document {
                name: name.to_string(),
                content_type: None,
                bytes: bytes.clone(),
            },
        );
    }
    pieces
}

fn three_more() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        (
            "cross-1.svg",
            view_of(
                &inputs(),
                Target::Section {
                    key: SectionKey::Cross { index: 0 },
                },
            )
            .into_bytes(),
        ),
        ("plan.svg", view_of(&inputs(), Target::Plan).into_bytes()),
        ("plan.png", png(1200, 600)),
    ]
}

/// `(part, media type)` of every attachment the reader of the domain finds on the lines.
fn attached(store: &energyml_storage::EpcStore, lines: &str) -> Vec<(String, Option<String>)> {
    let index = PackageIndex::build(store, store.revision());
    let listing = attachments::attachments_of(store, &index, &[lines]).remove(0);
    let mut found: Vec<(String, Option<String>)> = listing
        .pieces
        .iter()
        .map(|piece| match &piece.location {
            Location::Part(name) => (name.to_string(), piece.media_type.clone()),
            other => panic!("a part was expected: {other:?}"),
        })
        .collect();
    found.sort();
    found
}

fn lines_of(plan: &Plan) -> String {
    plan.objects
        .iter()
        .find(|o| o.qualified_type == "resqml22.PolylineSetRepresentation")
        .unwrap()
        .uuid
        .clone()
}

fn empty_store() -> energyml_storage::EpcStore {
    energyml_build::epc::open(None, "section.epc", Arc::new(energyml_schemas::all()))
        .expect("an empty package")
}

#[test]
fn further_drawings_are_attached_to_the_lines_in_the_folder_of_the_section() {
    let plan = plan(&bag_with_drawings(&three_more()), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let mut store = empty_store();
    let made = energyml_build::apply::apply_with(&plan, &mut store, &when(), false);
    assert!(
        made.invalid.is_empty(),
        "objects written against their schema: {:#?}",
        made.invalid
    );
    assert!(made.refused.is_empty(), "{:?}", made.refused);

    // Five drawings in all, every one in `media/sections/Section_A-A/`, its type
    // the one its bytes state.
    let folder = "media/sections/Section_A-A";
    let want: Vec<(String, Option<String>)> = [
        ("cross-1.svg", "image/svg+xml"),
        ("long.png", "image/png"),
        ("long.svg", "image/svg+xml"),
        ("plan.png", "image/png"),
        ("plan.svg", "image/svg+xml"),
    ]
    .into_iter()
    .map(|(file, kind)| (format!("{folder}/{file}"), Some(kind.to_string())))
    .collect();
    assert_eq!(attached(&store, &lines_of(&plan)), want);

    // The activity names a part per further drawing, in the order given.
    let parts: Vec<Value> = recorded(&plan)
        .into_iter()
        .filter(|(t, _)| t == "drawingPart")
        .map(|(_, v)| v)
        .collect();
    assert_eq!(
        parts,
        vec![
            json!(format!("{folder}/cross-1.svg")),
            json!(format!("{folder}/plan.svg")),
            json!(format!("{folder}/plan.png"))
        ]
    );
    // And each attachment is an output of the run.
    let activity = made.activity.as_ref().expect("an activity is written");
    let read = energyml_core::json::write_value(
        store
            .get_object(activity)
            .expect("the activity reads")
            .as_reflect(),
    );
    let outputs: Vec<&str> = read["Parameter"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["Title"] == "output")
        .filter_map(|p| p["DataObject"]["Uuid"].as_str())
        .collect();
    for attachment in plan
        .objects
        .iter()
        .filter(|o| o.qualified_type == "eml23.Attachment")
    {
        assert!(
            outputs.contains(&attachment.uuid.as_str()),
            "{} is not an output",
            attachment.title
        );
    }
}

/// A drawing is what its bytes say: an SVG named `.png` is attached as an SVG,
/// and a JPEG named `.svg` is refused - by its name, on the slot.
#[test]
fn a_further_drawing_is_typed_by_its_bytes_and_never_its_name() {
    let svg = view_of(&inputs(), Target::Plan).into_bytes();
    let made = plan(&bag_with_drawings(&[("plan.png", svg)]), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let part = made
        .media
        .iter()
        .find(|m| m.path.ends_with("/plan.png"))
        .expect("the part is written under the name it came with");
    assert_eq!(part.content_type, "image/svg+xml");

    let refusals = plan(
        &bag_with_drawings(&[("plan.svg", b"\xff\xd8\xff\xe0 a jpeg".to_vec())]),
        choices(),
    )
    .expect_err("accepted");
    let why = said(&refusals, "drawings");
    assert!(why.contains("plan.svg") && why.contains("neither a PNG"), "{why}");
    // A text that is not an SVG document is no drawing either.
    let refusals = plan(
        &bag_with_drawings(&[("notes.svg", b"<html><svg/></html>".to_vec())]),
        choices(),
    )
    .expect_err("accepted");
    assert!(said(&refusals, "drawings").contains("notes.svg"));
}

/// Two files of one name in the folder are one part: refused by their name,
/// the section's own SVG counting among them.
#[test]
fn two_drawings_of_the_same_file_name_are_refused_by_name() {
    let plan_view = view_of(&inputs(), Target::Plan).into_bytes();
    let refusals = plan(
        &bag_with_drawings(&[("plan.svg", plan_view.clone()), ("plan.svg", plan_view.clone())]),
        choices(),
    )
    .expect_err("accepted");
    let why = said(&refusals, "drawings");
    assert!(why.contains("plan.svg") && why.contains("same name"), "{why}");
    // The same name as the section's own SVG, from another folder of the machine.
    let refusals =
        plan(&bag_with_drawings(&[("exports/long.svg", plan_view)]), choices()).expect_err("accepted");
    assert!(said(&refusals, "drawings").contains("long.svg"));
}

/// An SVG that states another trace is the drawing of another section.
#[test]
fn a_drawing_of_another_trace_is_refused() {
    let mut elsewhere = inputs();
    elsewhere.trace = vec![[EAST, NORTH + 5000.0], [EAST + 1000.0, NORTH + 5000.0]];
    let other = view_of(&elsewhere, Target::Plan).into_bytes();
    let why = said(
        &plan(&bag_with_drawings(&[("plan.svg", other)]), choices()).expect_err("accepted"),
        "drawings",
    );
    assert!(why.contains("another trace"), "{why}");
    // An SVG that states no section view is a picture, attached as it is - and said.
    let picture = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>".to_vec();
    let made =
        plan(&bag_with_drawings(&[("legend.svg", picture)]), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(
        made.says
            .iter()
            .any(|n| n.key == "drawings" && n.detail.contains("states no section view"))
    );
}

/// Storing the section again - redrawn, so other bytes - writes the same
/// attachments over themselves: five, never ten.
#[test]
fn a_second_storing_replaces_every_drawing_it_attached() {
    let first = plan(&bag_with_drawings(&three_more()), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let mut files = three_more();
    files[1].1.extend_from_slice(b"<!-- redrawn -->");
    files[2].1 = png(1200, 601);
    let again = plan(&bag_with_drawings(&files), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let bytes = |plan: &Plan| plan.media.iter().map(|m| m.bytes.clone()).collect::<Vec<_>>();
    assert_ne!(bytes(&first), bytes(&again), "the test redraws");

    let uuids = |plan: &Plan| {
        let mut found: Vec<String> = plan
            .objects
            .iter()
            .filter(|o| o.qualified_type == "eml23.Attachment")
            .map(|o| o.uuid.clone())
            .collect();
        found.sort();
        found
    };
    assert_eq!(uuids(&first).len(), 5);
    assert_eq!(uuids(&first), uuids(&again));

    let mut store = empty_store();
    for plan in [&first, &again] {
        let made = energyml_build::apply::apply_with(plan, &mut store, &when(), false);
        assert!(
            made.invalid.is_empty(),
            "objects written against their schema: {:#?}",
            made.invalid
        );
        assert!(made.refused.is_empty(), "{:?}", made.refused);
    }
    assert_eq!(attached(&store, &lines_of(&first)).len(), 5);
}

/// Without `drawings`, nothing changes: the objects and their uuids are what
/// the section's own two files give, and every object written with more files
/// is the same one.
#[test]
fn without_further_drawings_the_plan_is_what_it_was() {
    let alone =
        plan(&bag(&drawing(&inputs()), Some(png(1200, 600))), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let with = plan(&bag_with_drawings(&three_more()), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let identity = |plan: &Plan| -> Vec<(String, String)> {
        plan.objects
            .iter()
            .map(|o| (o.qualified_type.clone(), o.uuid.clone()))
            .collect()
    };
    let (before, after) = (identity(&alone), identity(&with));
    assert_eq!(
        before.len(),
        9,
        "four frame objects, the feature, its interpretation, the lines, two attachments: {before:?}"
    );
    assert_eq!(after[..before.len()], before[..]);
    assert_eq!(after.len(), before.len() + 3);
    // The identity is the rule the section's own files always had - the lines
    // and what the file is - and a further drawing's is the lines and its file's
    // name: this is what makes a second storing replace.
    let lines = lines_of(&alone);
    let derived = |what: &str| {
        Identity::derived()
            .uuid(
                &format!("section-view:attachment:{}:{what}", lines.to_ascii_lowercase()),
                0,
            )
            .unwrap()
    };
    let attachments = |plan: &Plan| -> Vec<(String, String)> {
        plan.objects
            .iter()
            .filter(|o| o.qualified_type == "eml23.Attachment")
            .map(|o| (o.json["ContentUri"].as_str().unwrap().to_string(), o.uuid.clone()))
            .collect()
    };
    assert_eq!(
        attachments(&alone),
        vec![
            ("media/sections/Section_A-A/long.svg".to_string(), derived("SVG")),
            ("media/sections/Section_A-A/long.png".to_string(), derived("PNG")),
        ]
    );
    assert_eq!(
        attachments(&with)[2..],
        [
            (
                "media/sections/Section_A-A/cross-1.svg".to_string(),
                derived("file:cross-1.svg")
            ),
            (
                "media/sections/Section_A-A/plan.svg".to_string(),
                derived("file:plan.svg")
            ),
            (
                "media/sections/Section_A-A/plan.png".to_string(),
                derived("file:plan.png")
            ),
        ]
    );
    // No `drawingPart` and no note without them.
    assert!(recorded(&alone).iter().all(|(t, _)| t != "drawingPart"));
    assert!(alone.says.iter().all(|n| n.key != "drawings"));
    assert_eq!(
        recorded(&with).iter().filter(|(t, _)| t != "drawingPart").count(),
        recorded(&alone).len()
    );
}

/// The need: it is enough to list the activities that point to the right
/// activity template. A stored section is listed **by its activity's
/// template**: the section's activity is listed, with its name and the lines
/// it wrote; the activity of another operation is not - **even titled like a
/// section view**, since a title is anybody's to edit. To make it fail: test
/// the activity's title instead of its descriptor, or drop the template test.
#[test]
fn a_stored_section_is_listed_by_its_activitys_template_never_its_title() {
    let section =
        plan(&bag(&drawing(&inputs()), Some(png(1200, 600))), choices()).unwrap_or_else(|e| panic!("{e:?}"));
    let mut other = Registry::new()
        .plan(
            "crs",
            &Pieces::default(),
            &Choices::default().set("epsg", 2154),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|e| panic!("{:?}", e.refusals()));
    other.activity.title = section.activity.title.clone();
    let mut store = empty_store();
    for plan in [&section, &other] {
        let made = energyml_build::apply::apply_with(plan, &mut store, &when(), false);
        assert!(
            made.invalid.is_empty(),
            "objects written against their schema: {:#?}",
            made.invalid
        );
        assert!(made.refused.is_empty(), "{:?}", made.refused);
    }
    let activities = store
        .resources()
        .into_iter()
        .filter(|one| one.qualified_type.ends_with("Activity"))
        .count();
    assert_eq!(activities, 2, "both operations wrote their activity");

    let listed = energyml_build::read::stored_sections(&store);
    assert_eq!(listed.len(), 1, "{listed:?}");
    let lines = store
        .find_by_uuid(&lines_of(&section))
        .expect("the lines are written")
        .uri
        .clone();
    assert_eq!(listed[0].polyline.as_deref(), Some(lines.as_str()));
    assert_eq!(listed[0].name.as_deref(), Some("Section A-A'"));
    assert_eq!(listed[0].title, section.activity.title);
    // The drawing to reopen is the part the activity names, and it reads by
    // that name alone - no index of the package.
    assert_eq!(
        listed[0].drawings,
        vec![
            section
                .media
                .iter()
                .find(|m| m.path.ends_with(".svg"))
                .unwrap()
                .path
                .clone()
        ]
    );
    let key = format!("part:{}", listed[0].drawings[0]);
    let read = attachments::attachment_bytes(&store, &key).unwrap_or_else(|e| panic!("{key}: {e}"));
    let attachments::Fetched::Bytes { bytes, .. } = read else {
        panic!("{key} is held in the package")
    };
    assert!(
        String::from_utf8_lossy(&bytes).contains("<svg"),
        "{key} is the drawing"
    );
}
