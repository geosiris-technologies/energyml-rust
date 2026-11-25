//! The colour of a value: the file's map first, a palette else, a palette named winning over the file; never an
//! invented colour.

use energyml_domain::color::{ColorMap, Domain, Hsv, Interpolation, Paint, Rgba, Scaling, Stop};
use energyml_domain::palettes::Reading;
use energyml_views::{ColourDoor, ColourSource, ViewRefusal, value_colours};

use crate::fixtures::{NO_MAPS, Opened, package};

/// A door that gives every object a two-stop map, red to blue over 0 to 10.
struct RedToBlue;

impl ColourDoor for RedToBlue {
    fn paint(&self, _object_uri: &str) -> Result<Option<Paint>, String> {
        let stop = |index: f64, r: f64, b: f64| Stop {
            index,
            color: Hsv::from_rgba(Rgba { r, g: 0.0, b, a: 1.0 }),
        };
        Ok(Some(Paint::Mapped {
            map: Box::new(ColorMap {
                uuid: String::new(),
                title: None,
                continuous: Some((Domain::Rgb, Interpolation::Linear)),
                stops: vec![stop(0.0, 1.0, 0.0), stop(10.0, 0.0, 1.0)],
                null_color: None,
                above_max: None,
                below_min: None,
            }),
            scaling: Scaling {
                min_max: None,
                logarithmic: false,
                reverse: false,
            },
            value_vector_index: None,
        }))
    }
}

#[test]
fn without_a_map_the_default_palette_paints_and_says_so() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&NO_MAPS);
    let colours = value_colours(
        &cx,
        "test",
        Some("eml:///x"),
        None,
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect("colours");
    assert_eq!(colours.source, ColourSource::Palette("Viridis".to_string()));
    // The first colour Viridis publishes, quoted in the palette: #440154.
    assert_eq!(colours.sample(0.0), Some([0x44, 0x01, 0x54, 255]));
    assert_eq!(
        colours.sample(1.0),
        Some([0xfd, 0xe7, 0x25, 255]),
        "the last one at the top of the bounds"
    );
    // Break it: sample the ramp without the bounds, and 1.0 is not the top.
}

#[test]
fn the_files_map_paints_before_any_palette() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&RedToBlue);
    let colours = value_colours(
        &cx,
        "test",
        Some("eml:///x"),
        None,
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect("colours");
    assert_eq!(colours.source, ColourSource::File);
    assert_eq!(
        colours.sample(0.0),
        Some([255, 0, 0, 255]),
        "red at 0, by the file's own range"
    );
    assert_eq!(
        colours.sample(10.0),
        Some([0, 0, 255, 255]),
        "blue at 10, not at the bounds given"
    );
    // Break it: ask the palettes before the door.
}

#[test]
fn a_palette_named_wins_over_the_file_and_is_read_without_case() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&RedToBlue);
    let colours = value_colours(
        &cx,
        "test",
        Some("eml:///x"),
        Some("spectral"),
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect("colours");
    assert_eq!(colours.source, ColourSource::Palette("Spectral".to_string()));
    // Break it: ask the door first even when a palette is named.
}

#[test]
fn an_unknown_palette_or_one_of_another_reading_is_refused_by_name() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&NO_MAPS);
    let refused = value_colours(
        &cx,
        "test",
        None,
        Some("rainbow"),
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::Palette { why, .. } if why.contains("Viridis")),
        "{refused}"
    );
    let refused = value_colours(
        &cx,
        "test",
        None,
        Some("Set2"),
        (0.0, 1.0),
        false,
        Reading::Continuous,
    )
    .expect_err("refused");
    assert!(
        matches!(&refused, ViewRefusal::Palette { why, .. } if why.contains("another reading")),
        "{refused}"
    );
    // Break it: fall back on the default palette for a name that does not exist.
}

#[test]
fn a_value_the_map_has_no_colour_for_is_left_transparent() {
    let opened = Opened::of(package(50.0, &[20.0, 80.0]));
    let cx = opened.context(&NO_MAPS);
    let colours =
        value_colours(&cx, "test", None, None, (0.0, 1.0), false, Reading::Continuous).expect("colours");
    assert_eq!(
        colours.sample(f64::NAN),
        None,
        "a null value takes no colour: none is invented"
    );
    // Break it: sample a NaN as the bottom of the ramp.
}

const PROPERTY: &str = "c0000000-0000-4000-8000-000000000001";
const TABLE: &str = "c0000000-0000-4000-8000-000000000002";
const MAP: &str = "d0000000-0000-4000-8000-000000000001";
const SET: &str = "d0000000-0000-4000-8000-000000000009";

/// A package whose category property names a table, and a `ColorInformation` on that table (column 1) naming a
/// `DiscreteColorMap` of the keys 10 (red), 20 (green) and 35 (blue).
fn category_store() -> energyml_storage::EpcStore {
    use energyml_core::registry::DynObject;
    use energyml_domain::color::Decorated;
    use energyml_eml_2_3 as eml23;
    use energyml_resqml_2_2 as resqml22;

    use crate::fixtures::{FRAME, put};

    let integers = |values: &[i64]| eml23::IntegerXmlArray {
        count_per_value: 1,
        null_value: -1,
        values: values.to_vec(),
        ..Default::default()
    };
    let column = |title: &str, values: eml23::AbstractValueArray| eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(values),
        ..Default::default()
    };
    let hsv = |hue: f64| resqml22::HsvColor {
        alpha: 1.0,
        hue,
        saturation: 1.0,
        value: 1.0,
        title: None,
    };
    let entry = |index: i64, hue: f64| resqml22::DiscreteColorMapEntry { index, hsv: hsv(hue) };

    let mut store = package(50.0, &[0.0, 10.0, 20.0, 30.0]);
    put(
        &mut store,
        eml23::ColumnBasedTable {
            uuid: TABLE.to_string(),
            schema_version: "2.3".into(),
            key_column: vec![column(
                "STRA",
                eml23::AbstractValueArray::IntegerXmlArray(integers(&[10, 20, 35])),
            )],
            column: vec![column(
                "Nom_STRA",
                eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
                    count_per_value: 1,
                    values: vec!["Lutetian".into(), "Bartonian".into(), "Priabonian".into()],
                    ..Default::default()
                }),
            )],
            ..Default::default()
        },
    );
    put(
        &mut store,
        resqml22::DiscreteColorMap {
            uuid: MAP.to_string(),
            schema_version: "2.2".into(),
            entry: vec![entry(10, 0.0), entry(20, 120.0), entry(35, 240.0)],
            ..Default::default()
        },
    );
    let mut set = eml23::GraphicalInformationSet {
        uuid: SET.to_string(),
        schema_version: "2.3".into(),
        ..Default::default()
    };
    let decorated = |uuid: &str, qualified_type: &str| Decorated {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: "x".to_string(),
    };
    energyml_domain::color::set_colour_map(
        set.as_reflect_mut(),
        &[decorated(TABLE, "eml23.ColumnBasedTable")],
        &decorated(MAP, "resqml22.DiscreteColorMap"),
        &Scaling::default(),
        Some(1),
    )
    .expect("the entry is written");
    put(&mut store, set);
    put(
        &mut store,
        resqml22::DiscreteProperty {
            uuid: PROPERTY.to_string(),
            schema_version: "2.2".into(),
            indexable_element: eml23::IndexableElement::Intervals,
            value_count_per_indexable_element: vec![1],
            supporting_representation: eml23::DataObjectReference {
                uuid: FRAME.to_string(),
                qualified_type: "resqml22.WellboreFrameRepresentation".to_string(),
                ..Default::default()
            },
            values_for_patch: vec![resqml22::AbstractValueArray::IntegerXmlArray(integers(&[
                20, 10, 35,
            ]))],
            category_lookup: Some(eml23::DataObjectReference {
                uuid: TABLE.to_string(),
                qualified_type: "eml23.ColumnBasedTable".to_string(),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    store
}

fn category_colours(reading: Reading) -> (energyml_views::ValueColours, Opened) {
    let opened = Opened::of(category_store());
    let cache = energyml_cache::Cache::new(energyml_cache::Layers::all());
    let door = energyml_views::PackageColours {
        cache: &cache,
        store: &opened.store,
    };
    let property = crate::fixtures::uri("resqml22.DiscreteProperty", PROPERTY);
    let colours = {
        let cx = opened.context(&door);
        value_colours(&cx, "test", Some(&property), None, (0.0, 40.0), false, reading).expect("colours")
    };
    (colours, opened)
}

/// A `ColorInformation` on the **table of the categories** (issue #35: its `TargetObject` is the `CategoryLookup`, its
/// `ValueVectorIndex` a column, the entries of its `DiscreteColorMap` the table's keys) paints the values of the property
/// that names the table - through the package's own colours, the door the views ask - **by key**.
///
/// Break it: in `energyml_cache::colours::decide_in`, delete the `cascade.table_decision(..)` lookup (the palette then
/// paints and says so).
#[test]
fn a_category_property_is_painted_by_the_map_on_its_table() {
    let (colours, _opened) = category_colours(Reading::Categorical);
    assert!(
        matches!(colours.source, ColourSource::FileAndPalette(_)),
        "the table's map by key, a palette for a key it lacks: {:?}",
        colours.source
    );
    assert_eq!(colours.sample(10.0), Some([255, 0, 0, 255]));
    assert_eq!(colours.sample(20.0), Some([0, 255, 0, 255]));
    assert_eq!(colours.sample(35.0), Some([0, 0, 255, 255]));
}

/// A category is placed by its **exact key**: 25 is no key of the map, and it does not take the colour of 20 (the
/// nearest lower entry) - it takes the palette's, and the source says so. A continuous reading keeps the nearest-lower rule
/// of a measure.
///
/// Break it: in `Painting::sample`, answer `ByKey` by `map.sample(Some(value), &Scaling::default())`.
#[test]
fn a_category_is_placed_by_its_exact_key_and_a_missing_key_takes_the_palette() {
    let (colours, opened) = category_colours(Reading::Categorical);
    let green = Some([0, 255, 0, 255]);
    assert_ne!(colours.sample(25.0), green, "25 is not 20");
    assert!(colours.sample(25.0).is_some(), "the palette paints it");
    assert!(
        colours.source.says().contains("no entry"),
        "{}",
        colours.source.says()
    );

    // The same map read as a measure: the schema's own rule, nearest lower entry.
    let cache = energyml_cache::Cache::new(energyml_cache::Layers::all());
    let door = energyml_views::PackageColours {
        cache: &cache,
        store: &opened.store,
    };
    let cx = opened.context(&door);
    let property = crate::fixtures::uri("resqml22.DiscreteProperty", PROPERTY);
    let measure = value_colours(
        &cx,
        "test",
        Some(&property),
        None,
        (0.0, 40.0),
        false,
        Reading::Continuous,
    )
    .expect("colours");
    assert_eq!(measure.source, ColourSource::File);
    assert_eq!(
        measure.sample(25.0),
        green,
        "a measure takes the nearest lower entry"
    );
}

/// **One cascade**: what the scene asks (`Cache::decision`), what `energyml colors` and the views ask (`decisions_of`) say
/// the same of a category property - the colour map of its table, at the level of the table.
///
/// Break it: in `Cache::decision`'s walk (`decide_in`), skip the table (`table_of` returning `None`).
#[test]
fn the_scene_and_the_package_ask_the_same_cascade_of_a_category_property() {
    use energyml_cache::colours::{self, Options};
    use energyml_domain::color::Level;

    let opened = Opened::of(category_store());
    let cache = energyml_cache::Cache::new(energyml_cache::Layers::all());
    let property = crate::fixtures::uri("resqml22.DiscreteProperty", PROPERTY);
    let set = colours::graphical_set(&opened.store)
        .expect("the set reads")
        .expect("one set");

    let scene = cache
        .decision_at(
            &opened.store,
            &set,
            &crate::fixtures::frame_uri(),
            Some(&property),
            &[],
            &Options::default(),
        )
        .expect("the walk answers")
        .expect("the table colours it");
    assert_eq!(scene.level, Level::CategoryTable);

    let package = colours::decisions_of(&cache, &opened.store, &[property.as_str()], &Options::default())
        .expect("the sets read")
        .remove(0)
        .expect("the property reads")
        .expect("the table colours it");
    assert_eq!(package, scene, "one cascade, one answer");
}
