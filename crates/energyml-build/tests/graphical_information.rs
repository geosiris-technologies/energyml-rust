//! `graphical-information`: one `eml23.GraphicalInformationSet`, its entries
//! RESQML 2.2 `DefaultGraphicalInformation` and `ColorInformation`.
//!
//! The form of each assertion comes from the schemas and the reference guide,
//! cited where it is used:
//!
//! - `TargetObject` is `1..unbounded` (common v2.3 `GraphicalInformation.xsd`
//!   l. 19), and the entries of a set are the RESQML classes (l. 13, 27);
//! - a `DefaultGraphicalInformation` is *“Either for Feature, Interp or
//!   representation, marker”* (RESQML TRG 2.2 §4.10 p. 61), its
//!   `IndexableElementInfo` `1..unbounded` (`GraphicalInformationObject.xsd`
//!   l. 401) carrying the `ConstantColor` (l. 190) and the required
//!   `IsVisible` (l. 180);
//! - a `ColorInformation` *“Used for properties and property kinds and for
//!   geometry”* (TRG 2.2 §4.5 p. 56) names its `ColorMap`, its `MinMax`, the
//!   two required flags and the optional `ValueVectorIndex` (l. 257-288);
//! - an `HsvColor` is the four numbers of the HSV model the XSD points to
//!   (l. 547-578, *“See https://en.wikipedia.org/wiki/HSL_and_HSV”*): red is
//!   hue 0, blue hue 240, both at full saturation and value.
//!
//! The uuids, titles and colours are the test's own.
//!
//! # The JSON of a plan is read back twice, by readers that share nothing
//!
//! The entries of an eml 2.3 set are **subtrees** (`AnyXml`): the classes live
//! in RESQML, which eml cannot name. That is a known trap -
//! sixteen colours becoming sixteen
//! empty elements without an error. So [`read_back`] takes the plan's JSON
//! through the registry, the road `apply` takes, and reads each entry **twice**:
//! typed as the generated RESQML class its `xsi:type` names, with no departure
//! allowed (a required element missing or unreadable is one), and through the
//! domain's own narrow scanner, the reader the colour cascade stands on. The
//! values come back through both, or the test is red.

use energyml_build::operation::{Choices, Fitness, Identity, NewObject, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownProperty, Pieces, Ref, Source};
use energyml_core::registry::DynObject;
use energyml_domain::color::{GraphicalSet, Hsv, Information};
use energyml_domain::provenance::Parameter;
use energyml_eml_2_3::DataObjectReference;
use energyml_resqml_2_2 as resqml;
use serde_json::{Value, json};

const FEATURE: &str = "6f1e0c2a-3b4d-4e5f-8a6b-7c8d9e0f1a2b";
const INTERPRETATION: &str = "7a2f1d3b-4c5e-4f60-9b7c-8d9e0f1a2b3c";
const GRID: &str = "8b3a2e4c-5d6f-4071-8c8d-9e0f1a2b3c4d";
const MAP: &str = "9c4b3f5d-6e70-4182-9d9e-0f1a2b3c4d5e";
const POROSITY: &str = "ad5c4a6e-7f81-4293-8eaf-1a2b3c4d5e6f";
const PERMEABILITY: &str = "be6d5b7f-8092-43a4-9fb0-2b3c4d5e6f70";
const KIND: &str = "cf7e6c80-91a3-44b5-80c1-3c4d5e6f7081";
const SERIES: &str = "d08f7d91-a2b4-45c6-91d2-4d5e6f708192";

fn when() -> When {
    When {
        creation: "2026-09-24T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn object(uuid: &str, qualified_type: &str, title: &str) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, qualified_type, title)),
        ..Default::default()
    })
}

/// A property that states how many values each of its elements holds - what
/// the boundary resolves (`read::known_of_object`) - and whose values this
/// test does not read.
fn property(uuid: &str, title: &str, per_element: Vec<u64>) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(uuid, "resqml22.ContinuousProperty", title)),
        property: Some(KnownProperty {
            supporting: None,
            indexable: "cells".to_string(),
            value_count: per_element,
            timed: false,
            realizations: false,
            uom: None,
            kind: None,
            lookup: None,
            null_value: None,
            facets: Vec::new(),
            values: Err("not read by this test".to_string()),
        }),
        ..Default::default()
    })
}

/// The three levels of one horizon - what the colour rule paints alike
///.
fn cascade() -> Pieces {
    Pieces::empty()
        .with("targets", object(FEATURE, "resqml22.BoundaryFeature", "H3"))
        .with(
            "targets 2",
            object(INTERPRETATION, "resqml22.HorizonInterpretation", "H3"),
        )
        .with(
            "targets 3",
            object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
        )
}

/// A colour map and one property drawn through it.
fn mapped(per_element: Vec<u64>) -> Pieces {
    Pieces::empty()
        .with(
            "colourMap",
            object(MAP, "resqml22.ContinuousColorMap", "porosity, rgb"),
        )
        .with("mapped", property(POROSITY, "porosity", per_element))
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan(
            "graphical-information",
            pieces,
            &choices,
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.to_string())
}

fn titled(colours: Value) -> Choices {
    Choices::new()
        .set("title", "Lutetia - colours")
        .set("colours", colours)
}

fn untitled() -> Choices {
    Choices::new().set("title", "Lutetia - colours")
}

/// The one object a plan writes: the set.
fn the_set(plan: &Plan) -> &NewObject {
    let [set] = plan.objects.as_slice() else {
        panic!(
            "one object, the set: {:?}",
            plan.objects
                .iter()
                .map(|one| &one.qualified_type)
                .collect::<Vec<_>>()
        )
    };
    assert_eq!(set.qualified_type, "eml23.GraphicalInformationSet");
    set
}

/// The set of a plan, read back - see the module.
struct ReadBack {
    /// Each entry typed as the RESQML class its `xsi:type` names.
    typed: Vec<Box<dyn DynObject>>,
    /// The same set, through the domain's scanner.
    scanned: GraphicalSet,
}

fn read_back(plan: &Plan) -> ReadBack {
    let schemas = energyml_schemas::all();
    let document = energyml_core::json::read_value(&the_set(plan).json, &schemas)
        .unwrap_or_else(|why| panic!("the set does not read back: {why}"));
    let root = document.root.as_reflect();
    let set = root
        .downcast_ref::<energyml_eml_2_3::GraphicalInformationSet>()
        .expect("an eml 2.3 GraphicalInformationSet");
    let typed = set
        .graphical_information
        .iter()
        .enumerate()
        .map(|(at, raw)| {
            let (entry, departures) = raw
                .typed(&schemas)
                .unwrap_or_else(|| {
                    panic!(
                        "entry {at}: its `xsi:type` names no class this build knows: {:?}",
                        raw.attributes
                    )
                })
                .unwrap_or_else(|why| panic!("entry {at} does not read as the class it names: {why}"));
            assert!(
                departures.is_empty(),
                "entry {at} reads only with departures: {:?}",
                departures.lines()
            );
            entry
        })
        .collect();
    ReadBack {
        typed,
        scanned: GraphicalSet::read(root),
    }
}

fn default_at(read: &ReadBack, at: usize) -> &resqml::DefaultGraphicalInformation {
    let entry = read
        .typed
        .get(at)
        .unwrap_or_else(|| panic!("no entry {at}: {} written", read.typed.len()));
    entry
        .as_reflect()
        .downcast_ref::<resqml::DefaultGraphicalInformation>()
        .unwrap_or_else(|| {
            panic!(
                "entry {at} is a {}, not a DefaultGraphicalInformation",
                entry.as_reflect().type_info().name
            )
        })
}

fn colour_at(read: &ReadBack, at: usize) -> &resqml::ColorInformation {
    let entry = read
        .typed
        .get(at)
        .unwrap_or_else(|| panic!("no entry {at}: {} written", read.typed.len()));
    entry
        .as_reflect()
        .downcast_ref::<resqml::ColorInformation>()
        .unwrap_or_else(|| {
            panic!(
                "entry {at} is a {}, not a ColorInformation",
                entry.as_reflect().type_info().name
            )
        })
}

fn uuids(targets: &[DataObjectReference]) -> Vec<&str> {
    targets.iter().map(|one| one.uuid.as_str()).collect()
}

/// The constant colour of an entry covering the whole object, which the
/// schema requires to say whether it is visible.
fn whole_object_colour(entry: &resqml::DefaultGraphicalInformation) -> &resqml::HsvColor {
    match entry.indexable_element_info.as_slice() {
        [
            resqml::AbstractGraphicalInformationForIndexableElement::GraphicalInformationForWholeObject(
                whole,
            ),
        ] => {
            assert!(whole.is_visible, "written to be seen");
            whole.constant_color.as_ref().expect("a constant colour")
        }
        other => panic!("one whole-object element was expected: {other:?}"),
    }
}

/// An `HsvColor` as `#rrggbbaa`, through the domain's one conversion.
fn hex_of(colour: &resqml::HsvColor) -> String {
    Hsv {
        hue: colour.hue,
        saturation: colour.saturation,
        value: colour.value,
        alpha: colour.alpha,
        title: None,
    }
    .to_rgba()
    .to_hex()
}

// -- constant colours ---------------------------------------------------------

/// Each colour on the object it names, and the JSON the plan writes.
///
/// To break: pair the colours in another order, or give every object the
/// first - the two objects then carry the wrong hue, read by both readers.
#[test]
fn each_colour_lands_on_the_object_it_names() {
    let pieces = Pieces::empty()
        .with(
            "targets",
            object(INTERPRETATION, "resqml22.HorizonInterpretation", "H3"),
        )
        .with(
            "targets 2",
            object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
        );
    let plan = planned(&pieces, titled(json!(["#ff0000", "#0000ff"]))).expect("planned");

    let set = &the_set(&plan).json;
    assert_eq!(set["$type"], json!("eml23.GraphicalInformationSet"), "{set}");
    assert_eq!(set["schemaVersion"], json!("2.3"), "{set}");
    assert_eq!(set["Citation"]["Title"], json!("Lutetia - colours"), "{set}");
    let entries = set["GraphicalInformation"].as_array().expect("its entries");
    assert_eq!(entries.len(), 2, "{set}");
    for entry in entries {
        assert_eq!(
            entry["attributes"]["xsi:type"],
            json!("resqml:DefaultGraphicalInformation"),
            "{entry}"
        );
    }

    let read = read_back(&plan);
    let first = default_at(&read, 0);
    assert_eq!(uuids(&first.target_object), vec![INTERPRETATION]);
    let red = whole_object_colour(first);
    assert_eq!(
        (red.hue, red.saturation, red.value, red.alpha),
        (0.0, 1.0, 1.0, 1.0),
        "red in the HSV model"
    );
    let second = default_at(&read, 1);
    assert_eq!(uuids(&second.target_object), vec![GRID]);
    let blue = whole_object_colour(second);
    assert_eq!(
        (blue.hue, blue.saturation, blue.value, blue.alpha),
        (240.0, 1.0, 1.0, 1.0),
        "blue in the HSV model"
    );

    // The second reader, the one the cascade asks.
    let found = read.scanned.targeting(GRID);
    assert_eq!(found.len(), 1, "{:?}", read.scanned.notes);
    let Information::Default { elements, .. } = &found[0].information else {
        panic!("a default graphical information: {:?}", found[0].information)
    };
    assert_eq!(
        elements
            .first()
            .and_then(|one| one.constant_color.as_ref())
            .map(|colour| colour.to_rgba().to_hex()),
        Some("#0000ffff".to_string())
    );
}

/// The colour rule: the same `HsvColor` on the feature, the interpretation and the
/// representation - **three** entries, each naming its
/// own object, so that each level is found alone by the cascade.
///
/// To break: write one entry naming the three (the count and the targets go
/// red), or colour the levels differently (the equality does).
#[test]
fn the_cascade_puts_one_colour_on_the_feature_the_interpretation_and_the_representation() {
    let plan = planned(&cascade(), titled(json!(["#8b5a2b"]))).expect("planned");
    let read = read_back(&plan);
    assert_eq!(read.typed.len(), 3, "one entry per level");
    let levels = [FEATURE, INTERPRETATION, GRID];
    let colours: Vec<&resqml::HsvColor> = levels
        .iter()
        .enumerate()
        .map(|(at, uuid)| {
            let entry = default_at(&read, at);
            assert_eq!(
                uuids(&entry.target_object),
                vec![*uuid],
                "entry {at} names its own level, and only it"
            );
            whole_object_colour(entry)
        })
        .collect();
    assert_eq!(colours[0], colours[1]);
    assert_eq!(colours[1], colours[2]);
    assert_eq!(
        hex_of(colours[0]),
        "#8b5a2bff",
        "the colour asked, through HSV and back"
    );

    for uuid in levels {
        let found = read.scanned.targeting(uuid);
        assert_eq!(found.len(), 1, "{uuid}: {:?}", read.scanned.notes);
        assert!(
            matches!(found[0].information, Information::Default { .. }),
            "{:?}",
            found[0].information
        );
    }
}

/// `TargetObject` is `1..unbounded` (`GraphicalInformation.xsd` l. 19): one
/// entry may name them all, and then it holds one colour.
///
/// To break: ignore `shared` (three entries), or let it take several colours.
#[test]
fn shared_writes_one_entry_naming_every_object() {
    let plan = planned(&cascade(), titled(json!(["#8b5a2b"])).set("shared", true)).expect("planned");
    let read = read_back(&plan);
    assert_eq!(read.typed.len(), 1);
    assert_eq!(
        uuids(&default_at(&read, 0).target_object),
        vec![FEATURE, INTERPRETATION, GRID]
    );

    let Err(why) = planned(
        &cascade(),
        titled(json!(["#8b5a2b", "#8b5a2b", "#ff0000"])).set("shared", true),
    ) else {
        panic!("one shared entry and three colours")
    };
    assert!(why.contains("`shared`"), "{why}");
}

/// A colour that does not read is refused **by name**, all of them at once -
/// never drawn as some default colour nobody chose.
///
/// To break: a parser that falls back on grey, reads three digits, forgets the
/// `#`, or lets `u8::from_str_radix` take the sign in `#+f+f+f`; or a refusal
/// that stops at the first bad colour.
#[test]
fn a_colour_that_does_not_read_is_refused_by_name_and_never_replaced() {
    let pieces = Pieces::empty().with(
        "targets",
        object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
    );
    for bad in [
        "#12345g",
        "red",
        "#fff",
        "8b5a2b",
        "#8b5a2b0",
        "#+f+f+f",
        "hsv(30, 0.7, 0.5)",
    ] {
        let Err(why) = planned(&pieces, titled(json!([bad]))) else {
            panic!("`{bad}` must be refused")
        };
        assert!(
            why.contains("`colours`") && why.contains(&format!("`{bad}`")),
            "{bad}: {why}"
        );
    }

    let two = Pieces::empty()
        .with("targets", object(FEATURE, "resqml22.BoundaryFeature", "H3"))
        .with(
            "targets 2",
            object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
        );
    let Err(why) = planned(&two, titled(json!(["#12345g", "#zzzzzz"]))) else {
        panic!("two unreadable colours")
    };
    assert!(
        why.contains("`#12345g`") && why.contains("`#zzzzzz`"),
        "every one named: {why}"
    );
}

/// One colour for all the objects, or one each - anything else is refused
/// with both counts.
///
/// To break: pair what is there and drop the rest, or colour nothing.
#[test]
fn a_colour_for_all_or_one_per_object_and_nothing_between() {
    let why = planned(&cascade(), titled(json!(["#ff0000", "#0000ff"]))).unwrap_err();
    assert!(why.contains("2 colours for 3 objects"), "{why}");
    let why = planned(&cascade(), untitled()).unwrap_err();
    assert!(why.contains("`colours`") && why.contains("no colour"), "{why}");
    let why = planned(&mapped(vec![1]), titled(json!(["#ff0000"]))).unwrap_err();
    assert!(why.contains("no object in `targets`"), "{why}");
}

/// One object twice: coloured once and said; in two colours, refused - two
/// statements about one object in one set is the ambiguity the domain's
/// writer refuses to create (`set_constant_colour`).
#[test]
fn an_object_given_twice_is_coloured_once_and_two_colours_for_it_are_refused() {
    let twice = Pieces::empty()
        .with(
            "targets",
            object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
        )
        .with(
            "targets 2",
            object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
        );
    let plan = planned(&twice, titled(json!(["#ff0000"]))).expect("planned");
    assert_eq!(read_back(&plan).typed.len(), 1);
    assert!(
        plan.says.iter().any(|note| note.key == "twice"),
        "{:?}",
        plan.says
    );
    let why = planned(&twice, titled(json!(["#ff0000", "#0000ff"]))).unwrap_err();
    assert!(why.contains("given twice"), "{why}");
}

/// The same content is the same set; another colour or other objects are
/// another.
///
/// To break: derive the uuid from the title alone (the last two go red), or
/// from the colour as typed rather than as a colour (the first does).
#[test]
fn the_same_content_is_the_same_set_and_another_colour_another() {
    let uuid = |plan: &Plan| the_set(plan).uuid.clone();
    let one = planned(&cascade(), titled(json!(["#8b5a2b"]))).expect("planned");
    let again = planned(&cascade(), titled(json!(["#8B5A2BFF"]))).expect("planned");
    assert_eq!(
        uuid(&one),
        uuid(&again),
        "one colour, written two ways, is one content"
    );
    let other = planned(&cascade(), titled(json!(["#8b5a2c"]))).expect("planned");
    assert_ne!(uuid(&one), uuid(&other), "another colour is another set");
    let fewer = Pieces::empty().with(
        "targets",
        object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
    );
    let fewer = planned(&fewer, titled(json!(["#8b5a2b"]))).expect("planned");
    assert_ne!(uuid(&one), uuid(&fewer), "other objects, another set");
}

/// What was said is what the activity records, colour by colour - a list the
/// registry does not write.
#[test]
fn the_activity_records_every_colour_said() {
    let pieces = Pieces::empty()
        .with(
            "targets",
            object(INTERPRETATION, "resqml22.HorizonInterpretation", "H3"),
        )
        .with(
            "targets 2",
            object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
        );
    let plan = planned(&pieces, titled(json!(["#ff0000", "#0000FF"]))).expect("planned");
    let said: Vec<&str> = plan
        .activity
        .parameters
        .iter()
        .filter_map(|parameter| match parameter {
            Parameter::Text { title, value } if title == "colours" => Some(value.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(said, vec!["#ff0000", "#0000FF"]);
}

/// The title is proposed from the first object coloured.
#[test]
fn the_title_is_proposed_from_the_first_object() {
    let registry = Registry::new();
    let operation = registry.get("graphical-information").expect("registered");
    let title = operation
        .needs(&cascade())
        .into_iter()
        .find(|slot| slot.name == "title")
        .expect("a title slot");
    assert_eq!(title.suggested, Some(json!("H3 - colours")));
}

// -- colour maps --------------------------------------------------------------

/// A property drawn through a map: the map named, the range and the flags as
/// said (`GraphicalInformationObject.xsd` l. 257-288).
///
/// To break: drop `MinMax`, write the flags the other way, or name the wrong
/// map - each read by both readers.
#[test]
fn a_property_is_drawn_through_its_colour_map_with_the_range_and_flags_said() {
    let plan = planned(
        &mapped(vec![1]),
        untitled()
            .set("minimum", 0.05)
            .set("maximum", 0.35)
            .set("reverse", true),
    )
    .expect("planned");
    let read = read_back(&plan);
    assert_eq!(read.typed.len(), 1);
    let entry = colour_at(&read, 0);
    assert_eq!(uuids(&entry.target_object), vec![POROSITY]);
    let map = entry.color_map.as_ref().expect("the map is named");
    assert_eq!(
        (map.uuid.as_str(), map.qualified_type.as_str()),
        (MAP, "resqml22.ContinuousColorMap")
    );
    let range = entry.min_max.as_ref().expect("MinMax");
    assert_eq!((range.minimum, range.maximum), (0.05, 0.35));
    assert!(entry.use_reverse_mapping);
    assert!(!entry.use_logarithmic_mapping);
    assert_eq!(entry.value_vector_index, None);

    let found = read.scanned.targeting(POROSITY);
    assert_eq!(found.len(), 1, "{:?}", read.scanned.notes);
    let Information::Color {
        scaling,
        map,
        value_vector_index,
    } = &found[0].information
    else {
        panic!("a colour information: {:?}", found[0].information)
    };
    assert_eq!(map.as_deref(), Some(MAP));
    assert_eq!(scaling.min_max, Some((0.05, 0.35)));
    assert!(scaling.reverse && !scaling.logarithmic);
    assert_eq!(*value_vector_index, None);
}

/// The settings of a map, held to the schema: `MinMax` carries both bounds
/// and is an interval (l. 579-592); *“the log of the property values”*
/// (l. 269-273) has no value at zero; a setting of a map with no map is
/// refused rather than dropped.
#[test]
fn the_settings_of_a_map_are_held_to_the_schema() {
    let refused = |pieces: &Pieces, choices: Choices, slot: &str| {
        let Err(why) = planned(pieces, choices) else {
            panic!("must be refused on `{slot}`")
        };
        assert!(why.contains(&format!("`{slot}`")), "on `{slot}`: {why}");
    };
    refused(&mapped(vec![1]), untitled().set("minimum", 0.1), "maximum");
    refused(&mapped(vec![1]), untitled().set("maximum", 0.1), "minimum");
    refused(
        &mapped(vec![1]),
        untitled().set("minimum", 0.3).set("maximum", 0.3),
        "maximum",
    );
    refused(
        &mapped(vec![1]),
        untitled().set("minimum", 0.3).set("maximum", 0.1),
        "maximum",
    );
    refused(
        &mapped(vec![1]),
        untitled()
            .set("minimum", 0.0)
            .set("maximum", 1.0)
            .set("logarithmic", true),
        "logarithmic",
    );
    // The same mapping over a range of positive values is what the flag is for.
    let plan = planned(
        &mapped(vec![1]),
        untitled()
            .set("minimum", 0.01)
            .set("maximum", 1.0)
            .set("logarithmic", true),
    )
    .expect("a logarithmic mapping over positive values");
    assert!(colour_at(&read_back(&plan), 0).use_logarithmic_mapping);

    let no_map = Pieces::empty().with(
        "targets",
        object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
    );
    refused(
        &no_map,
        titled(json!(["#ff0000"])).set("reverse", true),
        "reverse",
    );
    refused(
        &no_map,
        titled(json!(["#ff0000"])).set("minimum", 0.0).set("maximum", 1.0),
        "minimum",
    );
}

/// `ValueVectorIndex` is an index into the **data** - the component of a
/// vector property - never into the map (`energyml_domain::color`). One past
/// the last component a property states is refused; where no count is stated,
/// it is written and said unchecked.
///
/// To break: compare against the map's stops, or skip the check.
#[test]
fn a_component_past_the_last_one_a_property_holds_is_refused() {
    let vector = Pieces::empty()
        .with(
            "colourMap",
            object(MAP, "resqml22.ContinuousColorMap", "permeability, hsv"),
        )
        .with("mapped", property(PERMEABILITY, "permeability", vec![3]));
    let why = planned(&vector, untitled().set("valueVectorIndex", 3)).unwrap_err();
    assert!(
        why.contains("`valueVectorIndex`") && why.contains("3 value(s) per element"),
        "{why}"
    );

    let plan = planned(&vector, untitled().set("valueVectorIndex", 2)).expect("the third component");
    assert_eq!(colour_at(&read_back(&plan), 0).value_vector_index, Some(2));
    assert!(
        !plan.says.iter().any(|note| note.key == "components-unchecked"),
        "{:?}",
        plan.says
    );

    let kind = Pieces::empty()
        .with(
            "colourMap",
            object(MAP, "resqml22.ContinuousColorMap", "permeability, hsv"),
        )
        .with("mapped", object(KIND, "eml23.PropertyKind", "permeability"));
    let plan = planned(&kind, untitled().set("valueVectorIndex", 2)).expect("a kind states no count");
    assert!(
        plan.says.iter().any(|note| note.key == "components-unchecked"),
        "{:?}",
        plan.says
    );
}

// -- what each slot takes -----------------------------------------------------

/// Each object is refused **on its slot**, by its class, with the way out.
///
/// A constant colour is for a feature, an interpretation, a representation or
/// a marker (TRG 2.2 §4.10 p. 61); a map draws a property, a property kind or
/// a representation (§4.5 p. 56); a colour map is one of the schema's two
/// (§4.1 p. 51); an element scope is a representation's - *“Each property is
/// "attached" to the indexable elements of a representation”* (§8 p. 232).
#[test]
fn an_object_of_the_wrong_kind_is_refused_on_its_slot() {
    let why = planned(
        &Pieces::empty().with("targets", property(POROSITY, "porosity", vec![1])),
        titled(json!(["#ff0000"])),
    )
    .unwrap_err();
    assert!(
        why.contains("`targets`") && why.contains("ContinuousProperty") && why.contains("`mapped`"),
        "{why}"
    );

    let feature = Pieces::empty()
        .with(
            "colourMap",
            object(MAP, "resqml22.ContinuousColorMap", "porosity, rgb"),
        )
        .with("mapped", object(FEATURE, "resqml22.BoundaryFeature", "H3"));
    let why = planned(&feature, untitled()).unwrap_err();
    assert!(
        why.contains("`mapped`") && why.contains("BoundaryFeature"),
        "{why}"
    );

    let series = Pieces::empty()
        .with("colourMap", object(SERIES, "eml23.TimeSeries", "years"))
        .with("mapped", property(POROSITY, "porosity", vec![1]));
    let why = planned(&series, untitled()).unwrap_err();
    assert!(why.contains("`colourMap`") && why.contains("TimeSeries"), "{why}");

    let why = planned(&cascade(), titled(json!(["#ff0000"])).set("scope", "nodes")).unwrap_err();
    assert!(
        why.contains("`scope`") && why.contains("BoundaryFeature"),
        "{why}"
    );
    let grid = Pieces::empty().with(
        "targets",
        object(GRID, "resqml22.Grid2dRepresentation", "H3 - grid"),
    );
    let plan = planned(&grid, titled(json!(["#ff0000"])).set("scope", "nodes")).expect("a grid has nodes");
    let read = read_back(&plan);
    assert!(
        matches!(
            default_at(&read, 0).indexable_element_info.as_slice(),
            [resqml::AbstractGraphicalInformationForIndexableElement::GraphicalInformationForNodes(_)]
        ),
        "{:?}",
        default_at(&read, 0).indexable_element_info
    );
}

/// A collection takes a constant colour - the house's rank 2 and 4 read it for
/// every object the collection holds - and
/// both readers find it on the collection. The standard's list stops at
/// *“Feature, Interp or representation, marker”* (TRG 2.2 §4.10 p. 61), so
/// what stays refused is checked beside it: a time series and a property.
///
/// To break: take the collection out of `decorable` (the plan is refused), or
/// let `decorable` accept anything (the `TimeSeries` refusal goes red).
#[test]
fn a_collection_takes_a_constant_colour_and_both_readers_find_it() {
    const FOLDER: &str = "e19a8eb2-b3c5-47d7-82e3-5e6f70819293";
    let pieces = Pieces::empty().with(
        "targets",
        object(FOLDER, "eml23.DataobjectCollection", "case 2 - collection"),
    );
    let plan = planned(&pieces, titled(json!(["#f58231"]))).expect("a collection is a target");

    let read = read_back(&plan);
    let entry = default_at(&read, 0);
    assert_eq!(uuids(&entry.target_object), vec![FOLDER]);
    let found = read.scanned.targeting(FOLDER);
    assert_eq!(found.len(), 1, "{:?}", read.scanned.notes);
    let Information::Default { elements, .. } = &found[0].information else {
        panic!("a default graphical information: {:?}", found[0].information)
    };
    assert_eq!(
        elements
            .first()
            .and_then(|one| one.constant_color.as_ref())
            .map(|colour| colour.to_rgba().to_hex()),
        Some("#f58231ff".to_string())
    );

    // What is no feature, interpretation, representation, marker or collection stays refused.
    let series = Pieces::empty().with("targets", object(SERIES, "eml23.TimeSeries", "years"));
    let why = planned(&series, titled(json!(["#f58231"]))).unwrap_err();
    assert!(
        why.contains("`targets`") && why.contains("TimeSeries") && why.contains("collection"),
        "{why}"
    );

    // An element scope is a representation's: a collection has no nodes.
    let why = planned(&pieces, titled(json!(["#f58231"])).set("scope", "nodes")).unwrap_err();
    assert!(
        why.contains("`scope`") && why.contains("DataobjectCollection"),
        "{why}"
    );
}

/// Nothing to colour is an inaptitude, and so is a map with nothing to draw or
/// objects to draw with no map - while the cascade is fit, or this test could
/// only ever say *unfit*.
#[test]
fn nothing_to_colour_is_unfit_and_a_map_goes_with_what_it_draws() {
    let registry = Registry::new();
    let operation = registry.get("graphical-information").expect("registered");
    assert!(matches!(operation.fitness(&Pieces::empty()), Fitness::Unfit(_)));
    let lone_map = Pieces::empty().with(
        "colourMap",
        object(MAP, "resqml22.ContinuousColorMap", "porosity, rgb"),
    );
    assert!(matches!(operation.fitness(&lone_map), Fitness::Unfit(_)));
    let lone_property = Pieces::empty().with("mapped", property(POROSITY, "porosity", vec![1]));
    assert!(matches!(operation.fitness(&lone_property), Fitness::Unfit(_)));
    assert_eq!(operation.fitness(&cascade()), Fitness::Fit);
    assert_eq!(operation.fitness(&mapped(vec![1])), Fitness::Fit);
}
