//! `isocontours`, the lines and the bands — two ticks, at least one.
//!
//! The need: two ticks, one to know whether to create the
//! polylines in the model, and the other to know whether to create the property. At
//! least one of the two must be ticked. The bands are the contoured
//! values as a `ContinuousProperty` on the surface, a `ContinuousColorMap` of
//! flat bands starting at each level, and the `ColorInformation` entry joining
//! them in a `GraphicalInformationSet` — the package's own, or a new one.
//!
//! The objects are built here, never read from a file: what is asserted is the
//! form the writers give, and the values are the pyramid's.

use energyml_build::operation::{Applies, Choices, Identity, Plan, Purpose, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use energyml_domain::color::Hsv;
use energyml_geometry::geometry::{Geometry, Patch, Topology};

fn when() -> When {
    When { creation: "2026-09-28T00:00:00Z".into(), originator: "test".into() }
}

const SURFACE: &str = "bbbbbbbb-0000-4000-8000-000000000002";

/// A pyramid: four triangles round a top at z = 10, the corners at z = 0.
fn pyramid() -> Geometry {
    let points = vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0], [5.0, 5.0, 10.0]];
    Geometry {
        kind: "x",
        patches: vec![Patch {
            index: 0,
            node_offset: 0,
            points,
            topology: Topology::Triangles(vec![[0, 1, 4], [1, 2, 4], [2, 3, 4], [3, 0, 4]]),
            closed: Vec::new(),
        }],
    }
}

/// The pyramid as an object of the package, in a compound frame counting up.
fn surface(vertical_up: Option<bool>) -> Pieces {
    Pieces::empty().with("shape", Source::Shape { geometry: pyramid(), closed: None }).with(
        // Named as `read::in_slot` names it: the shape's companion.
        "shape (object)",
        Source::Object(Known {
            reference: Some(Ref::new(SURFACE, "resqml22.TriangulatedSetRepresentation", "The pyramid")),
            crs: Some(Ref::new("aaaaaaaa-0000-4000-8000-000000000001", "eml23.LocalEngineeringCompoundCrs", "The frame")),
            vertical_uom: Some("m".into()),
            vertical_up,
            node_count: Some(5),
            ..Default::default()
        }),
    )
}

fn hsv(hue: f64) -> Hsv {
    Hsv { hue, saturation: 1.0, value: 1.0, alpha: 1.0, title: None }
}

/// A ramp as the front end hands its palette: dense stops over 0..1.
fn with_colours(bag: Pieces) -> Pieces {
    bag.with("colours", Source::Ramp { levels: vec![0.0, 0.5, 1.0], colours: vec![hsv(0.0), hsv(120.0), hsv(240.0)] })
}

/// A graphical information set of the package, whole, as the boundary hands it.
fn set_of_the_package(uuid: &str, title: &str) -> Source {
    let set = energyml_eml_2_3::GraphicalInformationSet {
        uuid: uuid.into(),
        schema_version: "2.3".into(),
        citation: energyml_eml_2_3::Citation { title: title.into(), ..Default::default() },
        ..Default::default()
    };
    Source::Object(Known {
        reference: Some(Ref::new(uuid, "eml23.GraphicalInformationSet", title)),
        json: Some(energyml_core::json::write_value(&set)),
        ..Default::default()
    })
}

fn plan(bag: &Pieces, choices: Choices) -> Plan {
    Registry::new().plan("isocontours", bag, &choices, &Identity::derived(), &when()).unwrap_or_else(|why| panic!("{why}"))
}

fn of_type<'a>(plan: &'a Plan, qualified_type: &str) -> Vec<&'a energyml_build::operation::NewObject> {
    plan.objects.iter().filter(|one| one.qualified_type == qualified_type).collect()
}

#[test]
fn nothing_ticked_is_refused_by_name() {
    let why = Registry::new()
        .plan("isocontours", &surface(Some(true)), &Choices::new().set("lines", false), &Identity::derived(), &when())
        .expect_err("at least one of the two");
    assert!(why.message().contains("lines") && why.message().contains("bands"), "{why}");
}

#[test]
fn the_lines_alone_are_what_the_operation_always_wrote() {
    let written = plan(&surface(Some(true)), Choices::new().set("count", 4).set("title", "Isos"));
    assert_eq!(of_type(&written, "resqml22.PolylineSetRepresentation").len(), 1);
    assert!(of_type(&written, "resqml22.ContinuousProperty").is_empty(), "bands is off by default");
}

/// The bands alone: no polyline set, and the three objects — the property on
/// the surface with the pyramid's heights, the banded map, a new set holding
/// the entry that joins them. And **no title asked**: the lines' naming applies
/// only when the lines are written.
#[test]
fn the_bands_alone_write_the_property_its_map_and_the_entry() {
    let bag = with_colours(surface(Some(true)));
    let written = plan(&bag, Choices::new().set("count", 4).set("lines", false).set("bands", true));

    assert!(of_type(&written, "resqml22.PolylineSetRepresentation").is_empty(), "the lines are not written");
    assert!(!written.shapes.is_empty(), "the lines are still computed — a scene keeps them");
    assert!(written.shapes.iter().all(|made| made.object.is_none()), "and point at no written object");

    let property = of_type(&written, "resqml22.ContinuousProperty");
    assert_eq!(property.len(), 1);
    let property = property[0];
    assert_eq!(property.json["SupportingRepresentation"]["Uuid"], SURFACE);
    assert_eq!(property.json["IndexableElement"], "nodes");
    assert_eq!(property.json["Uom"], "m");
    // Up, so `elevation`, from the dictionary.
    assert_eq!(property.json["PropertyKind"]["Title"], "elevation");
    // The values are the contoured ones: the pyramid's heights, node by node.
    let dataset = written.datasets.iter().find(|one| one.path.contains(&property.uuid)).expect("the property's values");
    assert_eq!(dataset.dimensions, vec![5]);
    match &dataset.values {
        energyml_build::operation::Numbers::F64(values) => assert_eq!(values, &vec![0.0, 0.0, 0.0, 0.0, 10.0]),
        other => panic!("doubles, not {other:?}"),
    }

    let maps = of_type(&written, "resqml22.ContinuousColorMap");
    assert_eq!(maps.len(), 1);
    let sets = of_type(&written, "eml23.GraphicalInformationSet");
    assert_eq!(sets.len(), 1, "no set in the package: a new one");
    let entry = sets[0].json["GraphicalInformation"].to_string();
    assert!(entry.contains(&property.uuid), "the entry names the property: {entry}");
    assert!(entry.contains(&maps[0].uuid), "and draws it through the map: {entry}");
}

/// The first band takes the ramp's bottom colour and the last its top: the
/// ramp is read at the same place of its range as the band is of the levels.
#[test]
fn the_bands_run_the_ramp_from_bottom_to_top() {
    let written = plan(&with_colours(surface(Some(true))), Choices::new().set("count", 4).set("bands", true).set("title", "Isos"));
    let map = of_type(&written, "resqml22.ContinuousColorMap")[0];
    let entries = map.json["Entry"].as_array().expect("entries");
    let hue = |at: usize| entries[at]["Hsv"]["Hue"].as_f64().unwrap();
    assert_eq!(hue(0), 0.0, "the first band, the ramp's bottom");
    assert_eq!(hue(entries.len() - 1), 240.0, "the last band, the ramp's top");
}

/// The package's one set gets the entry, **under its own uuid** — `apply`
/// replaces — and keeps what it held.
#[test]
fn the_package_s_set_is_rewritten_with_the_entry_added() {
    let set_uuid = "cccccccc-0000-4000-8000-000000000003";
    let bag = with_colours(surface(Some(true))).with("colourSet", set_of_the_package(set_uuid, "The colours"));
    let written = plan(&bag, Choices::new().set("bands", true).set("title", "Isos"));
    let sets = of_type(&written, "eml23.GraphicalInformationSet");
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].uuid, set_uuid, "the package's set, not a new one");
    assert_eq!(sets[0].title, "The colours");
    let property = of_type(&written, "resqml22.ContinuousProperty")[0];
    assert!(sets[0].json["GraphicalInformation"].to_string().contains(&property.uuid));
}

/// Two sets: the standard does not say which decides a colour, so choosing
/// one would decide on somebody's data — refused, naming both.
#[test]
fn two_sets_are_refused_until_one_is_named() {
    let bag = with_colours(surface(Some(true)))
        .with("colourSet", set_of_the_package("cccccccc-0000-4000-8000-000000000003", "Couleurs A"))
        .with("colourSet 2", set_of_the_package("cccccccc-0000-4000-8000-000000000004", "Couleurs B"));
    let why = Registry::new()
        .plan("isocontours", &bag, &Choices::new().set("bands", true).set("title", "Isos"), &Identity::derived(), &when())
        .expect_err("which set");
    assert!(why.message().contains("Couleurs A") && why.message().contains("Couleurs B"), "{why}");
}

#[test]
fn a_frame_that_does_not_say_which_way_z_counts_asks_for_the_kind() {
    let why = Registry::new()
        .plan("isocontours", &with_colours(surface(None)), &Choices::new().set("bands", true).set("title", "Isos"), &Identity::derived(), &when())
        .expect_err("no kind to decide");
    assert!(why.message().contains("propertyKind"), "{why}");
    // Down is `depth`.
    let written = plan(&with_colours(surface(Some(false))), Choices::new().set("bands", true).set("title", "Isos"));
    assert_eq!(of_type(&written, "resqml22.ContinuousProperty")[0].json["PropertyKind"]["Title"], "depth");
}

#[test]
fn the_bands_without_colours_are_refused_by_name() {
    let why = Registry::new()
        .plan("isocontours", &surface(Some(true)), &Choices::new().set("bands", true).set("title", "Isos"), &Identity::derived(), &when())
        .expect_err("no palette is shipped");
    assert!(why.message().contains("colours"), "{why}");
}

/// A plan asked for its geometry writes nothing: the bands, objects only, are
/// neither computed nor refused — keeping the lines in a scene needs no palette.
#[test]
fn a_plan_for_the_geometry_asks_nothing_of_the_bands() {
    let written = Registry::new()
        .plan_for(
            "isocontours",
            &surface(Some(true)),
            &Choices::new().set("bands", true),
            &Identity::derived(),
            &when(),
            Purpose::Geometry,
        )
        .unwrap_or_else(|why| panic!("{why}"));
    assert!(!written.shapes.is_empty());
    assert!(of_type(&written, "resqml22.ContinuousProperty").is_empty());
}

/// The naming of the lines applies under `lines`, the bands' under `bands` —
/// declared, so the front end and the CLI hide the same fields.
#[test]
fn each_group_of_settings_applies_under_its_tick() {
    let slots = Registry::new().get("isocontours").unwrap().needs(&Pieces::empty());
    let applies = |name: &str| slots.iter().find(|slot| slot.name == name).unwrap_or_else(|| panic!("{name}")).applies.clone();
    assert_eq!(applies("title"), Applies::When { slot: "lines", is: vec!["true"] });
    assert_eq!(applies("propertyKind"), Applies::When { slot: "bands", is: vec!["true"] });
    assert_eq!(applies("count"), Applies::Always);
    // And a flag's value, a boolean on the wire, satisfies the word `true`.
    let under_lines = applies("title");
    assert!(under_lines.now(|_| true, |_| true, |_| Some(serde_json::Value::Bool(true))));
    assert!(!under_lines.now(|_| true, |_| true, |_| Some(serde_json::Value::Bool(false))));
}

/// `isocontours` declares its colours a **palette** — a ramp read over its own
/// range — and `colormap` does not: there a ramp's levels are the values, and
/// a form offering a palette would write a map over 0..1 that draws nothing
/// right.
#[test]
fn a_ramp_is_a_palette_only_where_the_operation_says_so() {
    let registry = Registry::new();
    let palette = |operation: &str, slot: &str| {
        let takes = registry.get(operation).unwrap().takes();
        let found = takes.iter().find(|one| one.name == slot).unwrap_or_else(|| panic!("{operation}: {slot}"));
        match &found.need {
            energyml_build::operation::Need::Input(takes) => takes.palette,
            other => panic!("an input, not {other:?}"),
        }
    };
    assert!(palette("isocontours", "colours"));
    assert!(!palette("colormap", "ramp"));
}

/// **Every band has a width**, the last one and the one below the first level
/// included: the map runs from the lowest value to the highest. The report:
/// 28/09: the coloured band did not show the last colour — the last
/// band was a single stop, painted on every value above the last level and
/// shown by no legend.
#[test]
fn the_map_covers_the_values_from_lowest_to_highest() {
    let written = plan(&with_colours(surface(Some(true))), Choices::new().set("count", 4).set("bands", true).set("title", "Isos"));
    let map = of_type(&written, "resqml22.ContinuousColorMap")[0];
    let entries = map.json["Entry"].as_array().expect("entries");
    let index = |at: usize| entries[at]["Index"].as_f64().unwrap();
    // The pyramid runs from 0 to 10.
    assert_eq!(index(0), 0.0, "from the lowest value");
    assert_eq!(index(entries.len() - 1), 10.0, "to the highest");
    // Two stops per band, the last closed too: an even count.
    assert_eq!(entries.len() % 2, 0, "every band closed: {entries:?}");
    let hue = |at: usize| entries[at]["Hsv"]["Hue"].as_f64().unwrap();
    assert_eq!(hue(entries.len() - 2), hue(entries.len() - 1), "the last band is one colour over its width");
}

