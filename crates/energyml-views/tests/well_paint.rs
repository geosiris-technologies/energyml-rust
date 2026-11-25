//! `paint_wells`: the colours along a borehole as a section keeps it, by one door.
//!
//! Every package is fabricated in memory: one well (a feature and its interpretation) with two frames of one
//! trajectory - the frame a section keeps, **A**, and the frame that holds the property, **B** - so the values reach
//! the well's nodes only by measured depth. The expected colours are worked by hand from the colour map put on the
//! table of the categories (key 10 red, 20 green, 35 blue: the hues 0, 120 and 240 at full saturation and value),
//! and from the values the frame B states.

use energyml_core::xml::XmlText;
use energyml_domain::color::{Decorated, Scaling};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;
use energyml_section::model::{Blend, ObjectId, Side, View, WellColouring, WellItem};
use energyml_views::{PackageColours, PaintAsk, PaintLegend, ViewRefusal, WellOutcome, paint_wells};

use crate::fixtures::{NO_MAPS, Opened, TRAJECTORY, package, put, uri};

const FEATURE: &str = "a1000000-0000-4000-8000-000000000001";
const INTERPRETATION: &str = "a1000000-0000-4000-8000-000000000002";
const FRAME_A: &str = "a1000000-0000-4000-8000-0000000000a1";
const FRAME_B: &str = "a1000000-0000-4000-8000-0000000000b1";
const STRATI: &str = "a1000000-0000-4000-8000-0000000000c1";
const STRATI_BIS: &str = "a1000000-0000-4000-8000-0000000000c2";
const POROSITY: &str = "a1000000-0000-4000-8000-0000000000c3";
const FEATURE_2: &str = "a2000000-0000-4000-8000-000000000001";
const INTERPRETATION_2: &str = "a2000000-0000-4000-8000-000000000002";
const FRAME_C: &str = "a2000000-0000-4000-8000-0000000000a1";
const TRAJECTORY_2: &str = "a3000000-0000-4000-8000-000000000001";
const KIND: &str = "a1000000-0000-4000-8000-0000000000d1";
const TABLE: &str = "c0000000-0000-4000-8000-000000000002";
const MAP: &str = "d0000000-0000-4000-8000-000000000001";
const SET: &str = "d0000000-0000-4000-8000-000000000009";

fn reference(uuid: &str, qualified_type: &str, title: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

fn doubles(values: &[f64]) -> eml23::AbstractFloatingPointArray {
    eml23::AbstractFloatingPointArray::FloatingPointXmlArray(eml23::FloatingPointXmlArray {
        count_per_value: 1,
        values: values.to_vec(),
        ..Default::default()
    })
}

fn integers(values: &[i64]) -> eml23::IntegerXmlArray {
    eml23::IntegerXmlArray {
        count_per_value: 1,
        null_value: -1,
        values: values.to_vec(),
        ..Default::default()
    }
}

fn feature(uuid: &str, title: &str) -> resqml22::WellboreFeature {
    resqml22::WellboreFeature {
        uuid: uuid.to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: title.to_string(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn interpretation(uuid: &str, of: &str) -> resqml22::WellboreInterpretation {
    resqml22::WellboreInterpretation {
        uuid: uuid.to_string(),
        schema_version: "2.2".into(),
        interpreted_feature: reference(of, "resqml22.WellboreFeature", "a well"),
        is_drilled: true,
        ..Default::default()
    }
}

/// A frame of the well `of` (an interpretation), on the trajectory `trajectory`.
fn frame(
    uuid: &str,
    title: &str,
    of: &str,
    trajectory: &str,
    mds: &[f64],
) -> resqml22::WellboreFrameRepresentation {
    resqml22::WellboreFrameRepresentation {
        uuid: uuid.to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: title.to_string(),
            ..Default::default()
        },
        represented_object: Some(reference(
            of,
            "resqml22.WellboreInterpretation",
            "an interpretation",
        )),
        node_count: mds.len() as i64,
        node_md: Some(doubles(mds)),
        trajectory: reference(
            trajectory,
            "resqml22.WellboreTrajectoryRepresentation",
            "a trajectory",
        ),
        ..Default::default()
    }
}

/// A discrete property of a frame, on its intervals, naming the table of the categories.
fn category(uuid: &str, title: &str, on: &str, codes: &[i64]) -> resqml22::DiscreteProperty {
    resqml22::DiscreteProperty {
        uuid: uuid.to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: title.to_string(),
            ..Default::default()
        },
        indexable_element: eml23::IndexableElement::Intervals,
        value_count_per_indexable_element: vec![1],
        supporting_representation: reference(on, "resqml22.WellboreFrameRepresentation", "a frame"),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerXmlArray(integers(codes))],
        category_lookup: Some(reference(TABLE, "eml23.ColumnBasedTable", "strata")),
        property_kind: reference(KIND, "eml23.PropertyKind", "[Strati] code"),
        ..Default::default()
    }
}

/// The table of the categories, a `DiscreteColorMap` of its keys, and the `ColorInformation` that puts the map on the table.
fn colours_of_the_table(store: &mut energyml_storage::EpcStore) {
    use energyml_core::registry::DynObject;

    let column = |title: &str, values: eml23::AbstractValueArray| eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(values),
        ..Default::default()
    };
    let entry = |index: i64, hue: f64| resqml22::DiscreteColorMapEntry {
        index,
        hsv: resqml22::HsvColor {
            alpha: 1.0,
            hue,
            saturation: 1.0,
            value: 1.0,
            title: None,
        },
    };
    put(
        store,
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
        store,
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
    put(store, set);
}

/// One well whose frame B holds the strata, and whose frame A is the one a section keeps.
///
/// Frame B: nodes at MD 0, 10, 20, 30 and the codes 20, 10, 35 on its three intervals. Frame A: nodes at MD 0, 5, 15,
/// 25, 30, with nothing of its own.
fn well_with_strata() -> energyml_storage::EpcStore {
    let mut store = package(50.0, &[0.0, 100.0]);
    put(&mut store, feature(FEATURE, "a well"));
    put(&mut store, interpretation(INTERPRETATION, FEATURE));
    put(
        &mut store,
        frame(
            FRAME_A,
            "frame A",
            INTERPRETATION,
            TRAJECTORY,
            &[0.0, 5.0, 15.0, 25.0, 30.0],
        ),
    );
    put(
        &mut store,
        frame(
            FRAME_B,
            "frame B",
            INTERPRETATION,
            TRAJECTORY,
            &[0.0, 10.0, 20.0, 30.0],
        ),
    );
    colours_of_the_table(&mut store);
    put(&mut store, category(STRATI, "Strati", FRAME_B, &[20, 10, 35]));
    store
}

/// The well the section keeps: the object is frame A, five nodes going down, at the MDs of frame A.
fn kept(frame_uuid: &str, title: &str) -> WellItem {
    let md = vec![0.0, 5.0, 15.0, 25.0, 30.0];
    WellItem {
        object: ObjectId {
            store: "s1".to_string(),
            uri: uri("resqml22.WellboreFrameRepresentation", frame_uuid),
            uuid: frame_uuid.to_string(),
            title: title.to_string(),
            qualified_type: "resqml22.WellboreFrameRepresentation".to_string(),
        },
        s: 0.0,
        d: 0.0,
        side: Side::Left,
        head: [0.0, 0.0],
        path: md.iter().map(|md| [0.0, -md]).collect(),
        md: Some(md),
        intervals: Vec::new(),
        strata: None,
        merged: Vec::new(),
    }
}

fn ask(title: &str) -> PaintAsk<'_> {
    PaintAsk { title, palette: None }
}

/// The values of a property of **another frame of the well** are carried by MD onto the nodes the section kept, and
/// each node is coloured by the colour map put on the table of the categories, by its exact key: 20 green, 10 red, 35
/// blue. The legend names each category by its label and its colour. Applied to a view, the wells are coloured by what
/// the scene paints and the legend of categories is set.
///
/// Break it: in `resolve`, hand `carry` the frame's own depths (`&frames[0].mds`) instead of the section's (`mds`): the
/// nodes take other values.
#[test]
fn a_category_of_another_frame_is_carried_by_md_and_coloured_by_its_table() {
    let opened = Opened::of(well_with_strata());
    let cache = energyml_cache::Cache::new(energyml_cache::Layers::all());
    let door = PackageColours {
        cache: &cache,
        store: &opened.store,
    };
    let cx = opened.context(&door);
    let well = kept(FRAME_A, "frame A");
    let answer = paint_wells(&cx, &[&well], &ask("strati")).expect("the wells are asked");

    assert_eq!(answer.painted(), 1, "{:?}", answer.results);
    let (key, paint) = &answer.paints[0];
    assert_eq!(*key, well.object.key());
    assert_eq!(paint.blend, Blend::Net, "a code does not run into the next");
    assert_eq!(paint.nodes.len(), 5, "one colour per node the section kept");
    // MD 0 and 5 are in the first interval (20), MD 15 in the second (10), MD 25 in the third (35).
    assert_eq!(
        paint.nodes[..4],
        ["#00ff00ff", "#00ff00ff", "#ff0000ff", "#0000ffff"]
    );

    let Some(PaintLegend::Categories(legend)) = &answer.legend else {
        panic!("a legend of categories: {:?}", answer.legend)
    };
    let said: Vec<(&str, &str)> = legend
        .entries
        .iter()
        .map(|one| (one.label.as_str(), one.colour.as_str()))
        .collect();
    assert_eq!(
        said,
        [
            ("Lutetian", "#ff0000"),
            ("Bartonian", "#00ff00"),
            ("Priabonian", "#0000ff")
        ]
    );
    assert!(
        answer.notes.iter().any(|note| note.contains("by key")),
        "the colours' source is said: {:?}",
        answer.notes
    );

    let mut view = View::new(
        energyml_section::model::Target::Section {
            key: energyml_section::model::SectionKey::Long,
        },
        [800.0, 400.0],
    );
    answer.apply(&mut view);
    assert_eq!(view.well_colouring, WellColouring::Scene);
    assert!(
        view.strata_for_unpainted,
        "the wells the scene paints nothing along keep their strata"
    );
    assert_eq!(view.paints.len(), 1);
    assert!(view.category_legend.is_some() && view.ramp_legend.is_none());
}

/// Two properties of different titles fit what was asked on one well: the well is refused, **naming both**, and no
/// colour is chosen between them. The view is left as it was.
///
/// Break it: in `resolve`, drop the check on the number of titles (`titles.len() > 1`): the first property then paints
/// the well without a word.
#[test]
fn two_properties_that_fit_are_refused_and_listed() {
    let mut store = well_with_strata();
    put(
        &mut store,
        category(STRATI_BIS, "Strati bis", FRAME_B, &[10, 10, 10]),
    );
    let opened = Opened::of(store);
    let cx = opened.context(&NO_MAPS);
    let well = kept(FRAME_A, "frame A");
    let answer = paint_wells(&cx, &[&well], &ask("strati")).expect("the wells are asked");

    assert_eq!(answer.painted(), 0);
    let refused = answer.refused();
    assert_eq!(refused.len(), 1, "{:?}", answer.results);
    assert!(
        refused[0].1.contains("`Strati`") && refused[0].1.contains("`Strati bis`"),
        "both are named: {}",
        refused[0].1
    );
    assert!(answer.paints.is_empty());
    let mut view = View::new(
        energyml_section::model::Target::Section {
            key: energyml_section::model::SectionKey::Long,
        },
        [800.0, 400.0],
    );
    let before = view.well_colouring;
    answer.apply(&mut view);
    assert_eq!(
        view.well_colouring, before,
        "nothing was painted: the view is as it was"
    );
    assert!(view.paints.is_empty());

    // The more exact title chooses: the piece `bis` fits one property alone.
    let chosen = paint_wells(&cx, &[&well], &ask("bis")).expect("the wells are asked");
    assert_eq!(chosen.painted(), 1, "{:?}", chosen.results);
}

/// A well none of whose properties fits keeps its colour and is **counted**; the well that has the property is painted
/// all the same, and the results come in the order the wells were given.
///
/// Break it: in `resolve`, answer `Err` where the well has no property (instead of `Ok(None)`): the count of wells
/// without the property is then a count of refusals.
#[test]
fn a_well_without_the_property_is_counted_and_keeps_its_colour() {
    let mut store = well_with_strata();
    put(&mut store, feature(FEATURE_2, "another well"));
    put(&mut store, interpretation(INTERPRETATION_2, FEATURE_2));
    put(
        &mut store,
        frame(
            FRAME_C,
            "frame C",
            INTERPRETATION_2,
            TRAJECTORY,
            &[0.0, 10.0, 20.0, 30.0],
        ),
    );
    let opened = Opened::of(store);
    let cache = energyml_cache::Cache::new(energyml_cache::Layers::all());
    let door = PackageColours {
        cache: &cache,
        store: &opened.store,
    };
    let cx = opened.context(&door);
    let bare = kept(FRAME_C, "frame C");
    let with = kept(FRAME_A, "frame A");
    let answer = paint_wells(&cx, &[&bare, &with], &ask("STRATI")).expect("the wells are asked");

    assert_eq!(answer.painted(), 1);
    assert_eq!(answer.without_property(), 1);
    assert!(answer.refused().is_empty());
    assert_eq!(answer.results[0].well, "frame C", "the order given");
    assert_eq!(answer.results[0].outcome, WellOutcome::WithoutProperty);
    assert!(matches!(answer.results[1].outcome, WellOutcome::Painted { .. }));
    assert_eq!(
        answer.paints.len(),
        1,
        "the bare well has no paint: it keeps its colour"
    );

    // An empty title asks for nothing.
    assert!(matches!(
        paint_wells(&cx, &[&with], &ask("  ")),
        Err(ViewRefusal::Computing { .. })
    ));
}

/// Two frames whose depths cannot be compared (two trajectories, no datum stated on either) refuse the well **by
/// name**: the values are never shifted, and the well keeps its colour.
///
/// Break it: in `resolve`, build the reference of the frame B from the target's (`&reference` for every stated frame):
/// the depths are then compared with themselves and the well is painted.
#[test]
fn depths_that_cannot_be_compared_refuse_the_well_by_name() {
    let mut store = package(50.0, &[0.0, 100.0]);
    put(&mut store, feature(FEATURE, "a well"));
    put(&mut store, interpretation(INTERPRETATION, FEATURE));
    put(
        &mut store,
        resqml22::WellboreTrajectoryRepresentation {
            uuid: TRAJECTORY_2.to_string(),
            schema_version: "2.2".into(),
            md_interval: eml23::MdInterval {
                md_min: 0.0,
                md_max: 100.0,
                uom: eml23::LengthUomExt::M,
                datum: None,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    put(
        &mut store,
        frame(
            FRAME_A,
            "frame A",
            INTERPRETATION,
            TRAJECTORY,
            &[0.0, 5.0, 15.0, 25.0, 30.0],
        ),
    );
    put(
        &mut store,
        frame(
            FRAME_B,
            "frame B",
            INTERPRETATION,
            TRAJECTORY_2,
            &[0.0, 10.0, 20.0, 30.0],
        ),
    );
    colours_of_the_table(&mut store);
    put(&mut store, category(STRATI, "Strati", FRAME_B, &[20, 10, 35]));
    let opened = Opened::of(store);
    let cx = opened.context(&NO_MAPS);
    let well = kept(FRAME_A, "frame A");
    let answer = paint_wells(&cx, &[&well], &ask("strati")).expect("the wells are asked");

    assert_eq!(answer.painted(), 0);
    let refused = answer.refused();
    assert_eq!(refused.len(), 1, "{:?}", answer.results);
    assert!(refused[0].1.contains("datum"), "{}", refused[0].1);
}

/// A **measure** is blended from node to node, coloured over one scale from the lowest value painted to the highest, and
/// its legend is a ramp with the property's name and unit. Between two nodes of frame B the value is interpolated (MD 5
/// is half way from 0.1 to 0.2).
///
/// Break it: in `paint_wells`, use `Blend::Net` for a measure: the paint is then read node by node.
#[test]
fn a_measure_is_blended_and_has_a_ramp_legend() {
    let mut store = package(50.0, &[0.0, 100.0]);
    put(&mut store, feature(FEATURE, "a well"));
    put(&mut store, interpretation(INTERPRETATION, FEATURE));
    put(
        &mut store,
        frame(
            FRAME_A,
            "frame A",
            INTERPRETATION,
            TRAJECTORY,
            &[0.0, 5.0, 15.0, 25.0, 30.0],
        ),
    );
    put(
        &mut store,
        frame(
            FRAME_B,
            "frame B",
            INTERPRETATION,
            TRAJECTORY,
            &[0.0, 10.0, 20.0, 30.0],
        ),
    );
    put(
        &mut store,
        resqml22::ContinuousProperty {
            uuid: POROSITY.to_string(),
            schema_version: "2.2".into(),
            citation: eml23::Citation {
                title: "Porosity".to_string(),
                ..Default::default()
            },
            indexable_element: eml23::IndexableElement::Nodes,
            value_count_per_indexable_element: vec![1],
            supporting_representation: reference(FRAME_B, "resqml22.WellboreFrameRepresentation", "frame B"),
            uom: eml23::UnitOfMeasureExt::from_xml_text("m3/m3").expect("a unit"),
            values_for_patch: vec![resqml22::AbstractValueArray::FloatingPointXmlArray(
                eml23::FloatingPointXmlArray {
                    count_per_value: 1,
                    values: vec![0.1, 0.2, 0.3, 0.4],
                    ..Default::default()
                },
            )],
            ..Default::default()
        },
    );
    let opened = Opened::of(store);
    let cx = opened.context(&NO_MAPS);
    let well = kept(FRAME_A, "frame A");
    let answer = paint_wells(&cx, &[&well], &ask("poros")).expect("the wells are asked");

    assert_eq!(answer.painted(), 1, "{:?}", answer.results);
    let (_, paint) = &answer.paints[0];
    assert_eq!(paint.blend, Blend::Smooth);
    // Carried: 0.1, 0.15, 0.25, 0.35, 0.4 - the lowest takes the first colour of the default palette, the highest the last.
    assert_eq!(paint.nodes[0], "#440154ff");
    assert_eq!(paint.nodes[4], "#fde725ff");
    assert!(
        paint.nodes.iter().all(|node| !node.ends_with("00")),
        "every node has a colour: {:?}",
        paint.nodes
    );
    let Some(PaintLegend::Ramp(legend)) = &answer.legend else {
        panic!("a ramp: {:?}", answer.legend)
    };
    assert_eq!(legend.title, "Porosity");
    assert_eq!(legend.uom.as_deref(), Some("m3/m3"));
    assert!((legend.min - 0.1).abs() < 1e-12 && (legend.max - 0.4).abs() < 1e-12);

    let mut view = View::new(
        energyml_section::model::Target::Section {
            key: energyml_section::model::SectionKey::Long,
        },
        [800.0, 400.0],
    );
    answer.apply(&mut view);
    assert!(view.ramp_legend.is_some() && view.category_legend.is_none());
}

// -- what a real package showed -------------------------------------------------------------

/// The categories the file gives **no colour for** are one colour each, in turn, whatever their number: three codes, three
/// colours of the palette (Set2's first three, by the rank of the code among the rows of the table) - not the first colour
/// and then the last one of a ramp stretched over bounds the categories have nothing to do with.
///
/// Break it: in `paint_categories`, colour with `value_colours` over `(0.0, 1.0)` again: the codes 20 and 35 then both take
/// the last colour of the palette.
#[test]
fn categories_without_a_colour_of_the_file_take_one_colour_each() {
    let opened = Opened::of(well_with_strata());
    let cx = opened.context(&NO_MAPS);
    let well = kept(FRAME_A, "frame A");
    let answer = paint_wells(&cx, &[&well], &ask("strati")).expect("the wells are asked");
    let (_, paint) = &answer.paints[0];
    // Codes 20, 20, 10, 35: the rows of the table are the keys 10, 20, 35, in turn Set2's first, second and third colours.
    assert_eq!(
        paint.nodes[..4],
        ["#fc8d62ff", "#fc8d62ff", "#66c2a5ff", "#8da0cbff"]
    );
    let Some(PaintLegend::Categories(legend)) = &answer.legend else {
        panic!("a legend of categories")
    };
    let colours: Vec<&str> = legend.entries.iter().map(|one| one.colour.as_str()).collect();
    assert_eq!(colours, ["#66c2a5", "#fc8d62", "#8da0cb"]);
}

/// A second well, with its own frame holding a category property of the title `Strati 2` and the kind `kind`.
fn two_wells(kind: &str) -> energyml_storage::EpcStore {
    const FRAME_D: &str = "a2000000-0000-4000-8000-0000000000b1";
    let mut store = well_with_strata();
    put(&mut store, feature(FEATURE_2, "another well"));
    put(&mut store, interpretation(INTERPRETATION_2, FEATURE_2));
    put(
        &mut store,
        frame(
            FRAME_C,
            "frame C",
            INTERPRETATION_2,
            TRAJECTORY,
            &[0.0, 5.0, 15.0, 25.0, 30.0],
        ),
    );
    put(
        &mut store,
        frame(
            FRAME_D,
            "frame D",
            INTERPRETATION_2,
            TRAJECTORY,
            &[0.0, 10.0, 20.0, 30.0],
        ),
    );
    let mut second = category(STRATI_BIS, "Strati 2", FRAME_D, &[10, 10, 35]);
    second.property_kind = reference(KIND, "eml23.PropertyKind", kind);
    put(&mut store, second);
    store
}

/// The legend of the wells is called by what they share - the title their property kind goes by - and by what was asked
/// when they do not agree; and what the colours come from is said **once**, with the number of properties, not once per
/// well.
///
/// Break it: in `paint_categories`, push the source's words for every property again: the note is then said twice.
#[test]
fn the_legend_is_named_by_the_kind_the_wells_share_and_the_notes_are_counted() {
    let opened = Opened::of(two_wells("[Strati] code"));
    let cx = opened.context(&NO_MAPS);
    let one = kept(FRAME_A, "frame A");
    let other = kept(FRAME_C, "frame C");

    let answer = paint_wells(&cx, &[&one, &other], &ask("strati")).expect("the wells are asked");
    assert_eq!(answer.painted(), 2, "{:?}", answer.results);
    let Some(PaintLegend::Categories(legend)) = &answer.legend else {
        panic!("a legend of categories")
    };
    assert_eq!(legend.title, "[Strati] code", "the kind both properties go by");
    let sources: Vec<&String> = answer
        .notes
        .iter()
        .filter(|note| note.contains("palette Set2"))
        .collect();
    assert_eq!(sources.len(), 1, "{:?}", answer.notes);
    assert!(sources[0].contains("2 properties"), "{}", sources[0]);

    // The second property goes by another kind: the legend is called by what was asked.
    let opened = Opened::of(two_wells("[Other] code"));
    let cx = opened.context(&NO_MAPS);
    let answer = paint_wells(&cx, &[&one, &other], &ask("strati")).expect("the wells are asked");
    let Some(PaintLegend::Categories(legend)) = &answer.legend else {
        panic!("a legend of categories")
    };
    assert_eq!(legend.title, "strati");
}

/// A package written against the draft of 2.2 - the colour information names its map by `DiscreteColorMap`, declares its
/// prefixes on itself, and puts the map on the table (`ValueVectorIndex` 1: the column `Column`, the key column being 0) - is
/// painted **by the file's colours**: each code takes the colour of its entry, the legend names it by the label of
/// its row, and the source says it is the file's map by key. Its hues are fractions of a turn, which the notes say.
///
/// Break it: in `read_raw_entry`, look for the map under `ColorMap` alone: the cascade then finds nothing on the table and the
/// palette paints.
#[test]
fn the_colours_of_a_package_written_against_the_draft_are_the_files() {
    let read = |content_type: &str, xml: &str| {
        energyml_schemas::all()
            .read_xml(content_type, xml.as_bytes())
            .expect("the object reads")
    };
    let mut store = package(50.0, &[0.0, 100.0]);
    put(&mut store, feature(FEATURE, "a well"));
    put(&mut store, interpretation(INTERPRETATION, FEATURE));
    put(
        &mut store,
        frame(
            FRAME_A,
            "frame A",
            INTERPRETATION,
            TRAJECTORY,
            &[0.0, 5.0, 15.0, 25.0, 30.0],
        ),
    );
    put(
        &mut store,
        frame(
            FRAME_B,
            "frame B",
            INTERPRETATION,
            TRAJECTORY,
            &[0.0, 10.0, 20.0, 30.0],
        ),
    );
    put(&mut store, category(STRATI, "Strati", FRAME_B, &[1, 0, 3]));
    let table = format!(
        r####"<eml:ColumnBasedTable xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" uuid="{TABLE}" schemaVersion="2.3" objectVersion="0">
  <eml:Citation><eml:Title>codes</eml:Title><eml:Originator>test</eml:Originator><eml:Creation>2021-05-18T22:31:17Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>
  <eml:Existence>actual</eml:Existence>
  <eml:Column><eml:ValueCountPerRow>1</eml:ValueCountPerRow>
    <eml:PropertyKind><eml:Uuid>5f78f66a-ed1b-4827-a868-beb989febb31</eml:Uuid><eml:QualifiedType>eml23.PropertyKind</eml:QualifiedType><eml:Title>code</eml:Title></eml:PropertyKind>
    <eml:Values xsi:type="eml:StringXmlArray"><eml:CountPerValue>1</eml:CountPerValue><eml:Values>Q</eml:Values><eml:Values>N1</eml:Values><eml:Values>Socle</eml:Values><eml:Values>Alter</eml:Values><eml:Values>UND</eml:Values></eml:Values>
  </eml:Column>
  <eml:KeyColumn><eml:ValueCountPerRow>1</eml:ValueCountPerRow>
    <eml:PropertyKind><eml:Uuid>2bcbe7d8-cd41-4758-a88f-242781e7f495</eml:Uuid><eml:QualifiedType>eml23.PropertyKind</eml:QualifiedType><eml:Title>classification index</eml:Title></eml:PropertyKind>
    <eml:Values xsi:type="eml:IntegerXmlArray"><eml:CountPerValue>1</eml:CountPerValue><eml:NullValue>-1</eml:NullValue><eml:Values>0 1 2 3 4</eml:Values></eml:Values>
  </eml:KeyColumn>
</eml:ColumnBasedTable>"####
    );
    // Hues as fractions of a turn, as this producer writes them: 0 red, 1/3 green, 2/3 blue, 1/6 yellow, 5/6 magenta.
    let entries: String = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0 / 6.0, 5.0 / 6.0]
        .iter()
        .enumerate()
        .map(|(index, hue)| {
            format!(
                "<resqml:Entry><resqml:index>{index}</resqml:index><resqml:Hsv><resqml:Alpha>1</resqml:Alpha>\
                 <resqml:Hue>{hue}</resqml:Hue><resqml:Saturation>1</resqml:Saturation><resqml:Value>1</resqml:Value>\
                 </resqml:Hsv></resqml:Entry>"
            )
        })
        .collect();
    let map = format!(
        r####"<resqml:DiscreteColorMap xmlns:resqml="http://www.energistics.org/energyml/data/resqmlv2" xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" uuid="{MAP}" schemaVersion="2.2" objectVersion="0">
  <eml:Citation><eml:Title>colours</eml:Title><eml:Originator>test</eml:Originator><eml:Creation>2021-05-18T22:31:17Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>
  <eml:Existence>actual</eml:Existence>
  {entries}
</resqml:DiscreteColorMap>"####
    );
    let set = format!(
        r####"<eml:GraphicalInformationSet xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" uuid="{SET}" schemaVersion="2.3" objectVersion="0">
  <eml:Citation><eml:Title>a set</eml:Title><eml:Originator>test</eml:Originator><eml:Creation>2021-05-18T22:34:10Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>
  <eml:Existence>actual</eml:Existence>
  <eml:GraphicalInformation xsi:type="resqmlv2:ColorInformation" xmlns:commonv2="http://www.energistics.org/energyml/data/commonv2" xmlns:resqmlv2="http://www.energistics.org/energyml/data/resqmlv2"><commonv2:TargetObject><commonv2:Uuid>{TABLE}</commonv2:Uuid><commonv2:ObjectVersion>0</commonv2:ObjectVersion><commonv2:QualifiedType>eml23.ColumnBasedTable</commonv2:QualifiedType><commonv2:Title>codes</commonv2:Title></commonv2:TargetObject><resqmlv2:UseLogarithmicMapping>false</resqmlv2:UseLogarithmicMapping><resqmlv2:UseReverseMapping>false</resqmlv2:UseReverseMapping><resqmlv2:ValueVectorIndex>1</resqmlv2:ValueVectorIndex><resqmlv2:DiscreteColorMap><commonv2:Uuid>{MAP}</commonv2:Uuid><commonv2:ObjectVersion>0</commonv2:ObjectVersion><commonv2:QualifiedType>resqml22.DiscreteColorMap</commonv2:QualifiedType><commonv2:Title>colours</commonv2:Title></resqmlv2:DiscreteColorMap>
  </eml:GraphicalInformation>
</eml:GraphicalInformationSet>"####
    );
    for (content_type, xml) in [
        ("application/x-eml+xml;version=2.3;type=ColumnBasedTable", table),
        ("application/x-resqml+xml;version=2.2;type=DiscreteColorMap", map),
        (
            "application/x-eml+xml;version=2.3;type=GraphicalInformationSet",
            set,
        ),
    ] {
        store.put(read(content_type, &xml)).expect("it goes in");
    }
    let opened = Opened::of(store);
    let cache = energyml_cache::Cache::new(energyml_cache::Layers::all());
    let door = PackageColours {
        cache: &cache,
        store: &opened.store,
    };
    let cx = opened.context(&door);
    let well = kept(FRAME_A, "frame A");
    let answer = paint_wells(&cx, &[&well], &ask("strati")).expect("the wells are asked");

    assert_eq!(answer.painted(), 1, "{:?}", answer.results);
    let (_, paint) = &answer.paints[0];
    // Codes 1, 0, 3 on the three intervals: green, red, yellow.
    assert_eq!(
        paint.nodes[..4],
        ["#00ff00ff", "#00ff00ff", "#ff0000ff", "#ffff00ff"]
    );
    let Some(PaintLegend::Categories(legend)) = &answer.legend else {
        panic!("a legend of categories")
    };
    let said: Vec<(&str, &str)> = legend
        .entries
        .iter()
        .map(|one| (one.label.as_str(), one.colour.as_str()))
        .collect();
    assert_eq!(said, [("Q", "#ff0000"), ("N1", "#00ff00"), ("Alter", "#ffff00")]);
    assert!(
        answer
            .notes
            .iter()
            .any(|note| note.contains("the file's map by key")),
        "{:?}",
        answer.notes
    );
    // The hues were read as fractions of a turn, and the notes of the paint say so (the cascade's own notes).
    assert!(
        answer
            .notes
            .iter()
            .any(|note| note.contains("hue read as a fraction of a turn")),
        "{:?}",
        answer.notes
    );
}

/// `well_values` is the first half of `paint_wells`, public: the values of the property the caller picked by its uri,
/// carried by MD onto the nodes the section kept, with the property and the frame each part comes from and the range
/// the frame states. No colour is chosen here.
///
/// Break it: in `well_values`, build `Part::frame_uri` from the well's own object (`well.object.uri`) instead of the
/// frame the property hangs on: the part names frame A, which holds no value.
#[test]
fn well_values_gives_the_carried_values_and_where_they_come_from() {
    use energyml_domain::frame_values::Value;
    use energyml_views::{Pick, well_values};

    let opened = Opened::of(well_with_strata());
    let cx = opened.context(&NO_MAPS);
    let members = energyml_domain::well_members::well_members_of_index(&opened.index);
    let well = kept(FRAME_A, "frame A");
    let property = vec![uri("resqml22.DiscreteProperty", STRATI)];
    let (values, _notes) = well_values(&cx, &members, &well, &Pick::Uris(&property))
        .expect("the well is read")
        .expect("it has the property");

    assert_eq!(values.property_uri, property[0]);
    assert_eq!(values.parts.len(), 1);
    assert_eq!(
        values.parts[0].frame_uri,
        uri("resqml22.WellboreFrameRepresentation", FRAME_B)
    );
    assert!(
        !values.continuous && values.categorical,
        "a code of a table of categories"
    );
    let codes: Vec<Option<f64>> = values
        .values
        .iter()
        .map(|value| value.as_ref().and_then(Value::as_f64))
        .collect();
    assert_eq!(codes[..4], [Some(20.0), Some(20.0), Some(10.0), Some(35.0)]);
    assert_eq!(values.range, Some((10.0, 35.0)));

    // A property the well does not have: not a refusal, a well without it.
    let other = vec![uri("resqml22.DiscreteProperty", STRATI_BIS)];
    assert!(
        well_values(&cx, &members, &well, &Pick::Uris(&other))
            .expect("read")
            .is_none()
    );
}

/// A well whose property holds nothing at its nodes says **why**, by cause and each counted: nodes outside every frame that
/// holds the property, and nodes the frame holds and gives no value (a null). Here the codes of the frame are all the null
/// value, and the well reaches below the frame.
///
/// Break it: in `well_values`, leave the count of the null nodes out of the refusal.
#[test]
fn a_property_of_nulls_is_refused_by_its_two_counts() {
    let mut store = package(50.0, &[0.0, 100.0]);
    put(&mut store, feature(FEATURE, "a well"));
    put(&mut store, interpretation(INTERPRETATION, FEATURE));
    put(
        &mut store,
        frame(
            FRAME_A,
            "frame A",
            INTERPRETATION,
            TRAJECTORY,
            &[0.0, 5.0, 15.0, 25.0, 40.0, 50.0],
        ),
    );
    put(
        &mut store,
        frame(
            FRAME_B,
            "frame B",
            INTERPRETATION,
            TRAJECTORY,
            &[0.0, 10.0, 20.0, 30.0],
        ),
    );
    colours_of_the_table(&mut store);
    // The null value of `integers` is -1.
    put(&mut store, category(STRATI, "Strati", FRAME_B, &[-1, -1, -1]));
    let opened = Opened::of(store);
    let cx = opened.context(&NO_MAPS);
    let mut well = kept(FRAME_A, "frame A");
    well.md = Some(vec![0.0, 5.0, 15.0, 25.0, 40.0, 50.0]);
    well.path = well.md.iter().flatten().map(|md| [0.0, -md]).collect();
    let answer = paint_wells(&cx, &[&well], &ask("strati")).expect("the wells are asked");

    let refused = answer.refused();
    assert_eq!(refused.len(), 1, "{:?}", answer.results);
    assert!(
        refused[0].1.contains("node(s) lie outside every frame")
            && refused[0].1.contains("have no value there (null)"),
        "{}",
        refused[0].1
    );
    assert!(
        refused[0].1.contains("2 of 6"),
        "the two nodes below the frame: {}",
        refused[0].1
    );
}
