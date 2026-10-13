//! A well's stratigraphy along its MD - sections, units, colours, refusals.
//!
//! Every object is built in memory from the generated types, after the
//! specification rather than after a package: RESQML TRG 2.2 §5.34 (p. 130)
//! for `IntervalStratigraphicUnits`, §6.45 for the rank it indexes into, the
//! common TRG 2.3 §9.1 for a category table's key column. The corpus holds no
//! frame mapped to units that a test could rely on, and a value measured in a
//! package would be a value the suite then defends.
//!
//! # What it would take to make each one fail
//!
//! - reading an index as a position **in the package**, or in title order,
//!   rather than in the rank's `StratigraphicUnits` (2.2) or by its declared
//!   `Index` (2.0.1) - the ranks below list their units out of both orders;
//! - reading a category code as a **row position** rather than a key - the
//!   keys below are `10, 20, 35`, so a position lookup finds no row for any;
//! - taking the reference's cached title over the unit's own - the ranks below
//!   cache a stale one;
//! - moving the node rule by one - the colours below differ from one section
//!   to the next, so a shifted node takes its neighbour's;
//! - drawing a null, an out-of-rank index or two ranks' disagreement instead
//!   of refusing it.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::color::{self, GraphicalSet, Rgba, Scope};
use energyml_domain::source::Prefetched;
use energyml_domain::well_strata::{self, Drawn, Known, Refusal, Source, Strata, Wanted};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_0_1 as resqml20;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::ArrayLocation;

const FRAME: &str = "f0000000-0000-4000-8000-000000000001";
const TRAJECTORY: &str = "f0000000-0000-4000-8000-000000000002";
const FEATURE: &str = "f0000000-0000-4000-8000-000000000003";
const RANK: &str = "a0000000-0000-4000-8000-000000000001";
const OTHER_RANK: &str = "a0000000-0000-4000-8000-000000000002";
const PRIABONIAN: &str = "b0000000-0000-4000-8000-000000000001";
const BARTONIAN: &str = "b0000000-0000-4000-8000-000000000002";
const LUTETIAN: &str = "b0000000-0000-4000-8000-000000000003";
const YPRESIAN: &str = "b0000000-0000-4000-8000-000000000004";
const PROPERTY: &str = "c0000000-0000-4000-8000-000000000001";
const TABLE: &str = "c0000000-0000-4000-8000-000000000002";
const SET: &str = "d0000000-0000-4000-8000-000000000001";
const CHART_SET: &str = "d0000000-0000-4000-8000-000000000002";

const RED: Rgba = Rgba {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const BLUE: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};
const GREEN: Rgba = Rgba {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
const GONE: Rgba = Rgba {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 0.0,
};

// -- building blocks ---------------------------------------------------------

/// No array lives outside the document here: every array is inline XML.
fn nowhere(_: &dyn Reflect) -> Vec<ArrayLocation> {
    Vec::new()
}

fn boxed<T: DynObject + 'static>(object: T) -> Box<dyn DynObject> {
    Box::new(object)
}

/// A package held in memory.
fn held(objects: Vec<Box<dyn DynObject>>) -> Prefetched {
    let mut cache = Prefetched::new();
    for object in objects {
        cache.insert(object, &nowhere);
    }
    cache
}

fn dor(uuid: &str, qualified_type: &str, title: &str) -> eml23::DataObjectReference {
    eml23::DataObjectReference {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: title.to_string(),
        ..Default::default()
    }
}

fn citation(title: &str) -> eml23::Citation {
    eml23::Citation {
        title: title.to_string(),
        ..Default::default()
    }
}

fn integers(values: &[i64]) -> eml23::IntegerXmlArray {
    eml23::IntegerXmlArray {
        count_per_value: 1,
        null_value: -1,
        values: values.to_vec(),
        ..Default::default()
    }
}

/// A 2.2 frame at these measured depths, with these mappings.
fn frame(
    mds: &[f64],
    maps: Vec<resqml22::IntervalStratigraphicUnits>,
) -> resqml22::WellboreFrameRepresentation {
    resqml22::WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
        citation: citation("F1 - frame"),
        node_count: mds.len() as i64,
        node_md: Some(eml23::AbstractFloatingPointArray::FloatingPointXmlArray(
            eml23::FloatingPointXmlArray {
                count_per_value: 1,
                values: mds.to_vec(),
                ..Default::default()
            },
        )),
        trajectory: dor(TRAJECTORY, "resqml22.WellboreTrajectoryRepresentation", "F1"),
        interval_stratigraphic_units: maps,
        ..Default::default()
    }
}

/// A 2.2 mapping: a jagged array, `ends` its cumulative lengths.
fn mapping(rank: &str, elements: &[i64], ends: &[i64]) -> resqml22::IntervalStratigraphicUnits {
    resqml22::IntervalStratigraphicUnits {
        unit_indices: eml23::JaggedArray {
            elements: Some(eml23::AbstractValueArray::IntegerXmlArray(integers(elements))),
            cumulative_length: Some(eml23::AbstractIntegerArray::IntegerXmlArray(integers(ends))),
        },
        stratigraphic_organization_interpretation: dor(
            rank,
            "resqml22.StratigraphicColumnRankInterpretation",
            "rank",
        ),
    }
}

/// A 2.2 rank listing these units **in this order** - the order an index
/// counts in. The references cache a stale title on purpose.
fn rank(uuid: &str, units: &[&str]) -> resqml22::StratigraphicColumnRankInterpretation {
    resqml22::StratigraphicColumnRankInterpretation {
        uuid: uuid.to_string(),
        citation: citation("stages"),
        ascending_ordering_criteria: resqml22::OrderingCriteria::Age,
        rank_in_stratigraphic_column: 1,
        stratigraphic_units: units
            .iter()
            .map(|unit| {
                dor(
                    unit,
                    "resqml22.StratigraphicUnitInterpretation",
                    "a stale cached title",
                )
            })
            .collect(),
        interpreted_feature: dor(FEATURE, "resqml22.RockVolumeFeature", "column"),
        ..Default::default()
    }
}

fn unit(uuid: &str, title: &str) -> Box<dyn DynObject> {
    Box::new(resqml22::StratigraphicUnitInterpretation {
        uuid: uuid.to_string(),
        citation: citation(title),
        interpreted_feature: dor(FEATURE, "resqml22.RockVolumeFeature", "rock"),
        ..Default::default()
    })
}

/// A `GraphicalInformationSet` giving these units these colours - written by
/// the writer the front end uses and read back by the reader, rather than built as
/// the reader's output: what is tested is then what a package would say.
fn graphics(colours: &[(&str, Rgba)]) -> Vec<(String, GraphicalSet)> {
    vec![(SET.to_string(), one_set(colours))]
}

/// One set, read - to hand several to [`Known::graphics`].
fn one_set(colours: &[(&str, Rgba)]) -> GraphicalSet {
    let mut set = eml23::GraphicalInformationSet::default();
    for (uuid, rgba) in colours {
        color::set_constant_colour(
            set.as_reflect_mut(),
            &[color::Decorated {
                uuid: uuid.to_string(),
                qualified_type: "resqml22.StratigraphicUnitInterpretation".to_string(),
                title: "unit".to_string(),
            }],
            Scope::WholeObject,
            *rgba,
            None,
        )
        .expect("the colour is written");
    }
    GraphicalSet::read(set.as_reflect())
}

fn units_of(strata: &Strata) -> Vec<Option<&str>> {
    strata
        .sections
        .iter()
        .map(|section| section.unit.map(|at| strata.units[at].title.as_str()))
        .collect()
}

/// The RGBA of each node, as hex.
fn node_hex(strata: &Strata, missing: Rgba) -> Vec<String> {
    strata
        .node_colours(missing)
        .chunks(4)
        .map(|rgba| format!("#{:02x}{:02x}{:02x}{:02x}", rgba[0], rgba[1], rgba[2], rgba[3]))
        .collect()
}

// -- IntervalStratigraphicUnits ------------------------------------------------

/// TRG 2.2 §5.34: *"Index of the stratigraphic unit per interval, of a given
/// stratigraphic column"*, through a jagged array, a null value where *"there
/// is no stratigraphic column"*. In 2.2 the rank's `StratigraphicUnits` has no
/// `Index`, so the index **is** the position in that list.
#[test]
fn sections_come_from_the_interval_units_through_the_rank() {
    // The rank lists Priabonian, Bartonian, Lutetian - youngest first, the
    // order `age` means. Not the order the uuids sort in, and not the order
    // the objects are added to the package below.
    let the_rank = rank(RANK, &[PRIABONIAN, BARTONIAN, LUTETIAN]);
    let well = frame(
        &[0.0, 10.0, 25.0, 40.0, 60.0],
        vec![mapping(RANK, &[0, 1, -1, 2], &[1, 2, 3, 4])],
    );
    let source = held(vec![
        unit(LUTETIAN, "Lutetian"),
        unit(BARTONIAN, "Bartonian"),
        unit(PRIABONIAN, "Priabonian"),
        boxed(the_rank),
        boxed(well.clone()),
    ]);
    let set = graphics(&[(PRIABONIAN, RED), (LUTETIAN, BLUE)]);
    let known = Known {
        graphics: &set,
        units: &[],
    };

    let strata = well_strata::of_frame(&well, &Wanted::IntervalUnits { rank: None }, &source, &known)
        .expect("the frame maps its intervals");

    assert_eq!(
        strata.source,
        Source::IntervalUnits {
            ranks: vec![RANK.to_string()]
        }
    );
    // The whole rank, in its order - a legend for every frame mapped through it
    // - and each title the unit's own, not the stale one the rank cached.
    let titles: Vec<&str> = strata.units.iter().map(|unit| unit.title.as_str()).collect();
    assert_eq!(titles, ["Priabonian", "Bartonian", "Lutetian"]);
    assert_eq!(
        strata.units.iter().map(|unit| unit.code).collect::<Vec<_>>(),
        [0.0, 1.0, 2.0]
    );

    assert_eq!(
        units_of(&strata),
        [Some("Priabonian"), Some("Bartonian"), None, Some("Lutetian")]
    );
    let spans: Vec<(f64, f64)> = strata
        .sections
        .iter()
        .map(|section| (section.top, section.base))
        .collect();
    assert_eq!(spans, [(0.0, 10.0), (10.0, 25.0), (25.0, 40.0), (40.0, 60.0)]);

    // The colour is the unit's own entry, and the object it came from is said.
    assert_eq!(
        strata.colour_of(&strata.sections[0]).map(Rgba::to_hex),
        Some("#ff0000ff".to_string())
    );
    assert_eq!(strata.units[0].colour_from.as_deref(), Some(PRIABONIAN));
    // Bartonian has none, and none is made up.
    assert_eq!(strata.units[1].colour, None);
    assert!(
        strata
            .notes
            .iter()
            .any(|note| note.contains("Bartonian") && note.contains("no colour")),
        "an uncoloured unit is said: {:?}",
        strata.notes
    );

    // Five nodes for four sections: each node takes the section it opens, the
    // deepest the one it closes. Bartonian and the null take the colour of
    // absent values - the caller's, transparent here.
    assert_eq!(
        node_hex(&strata, GONE),
        ["#ff0000ff", "#00000000", "#00000000", "#0000ffff", "#0000ffff"]
    );
    // And the caller's colour when it gives one.
    let grey = Rgba {
        r: 0.5,
        g: 0.5,
        b: 0.5,
        a: 1.0,
    };
    assert_eq!(node_hex(&strata, grey)[1], grey.to_hex());
}

/// A unit the file does not colour takes a colour of **the caller's** palette,
/// and the answer keeps it apart from the file's.
///
/// The rule is ours (`Strata::drawn`): the *k*-th uncoloured unit **in the
/// rank's order** takes the *k*-th colour. What would make it fail: counting
/// every unit rather than the uncoloured ones (Bartonian is the second unit,
/// so it would take the second colour), or counting in the sections' order
/// (the frame below names Ypresian before Bartonian).
#[test]
fn a_unit_the_file_does_not_colour_takes_the_callers_palette_and_says_so() {
    let the_rank = rank(RANK, &[PRIABONIAN, BARTONIAN, LUTETIAN, YPRESIAN]);
    let well = frame(
        &[0.0, 10.0, 20.0, 30.0],
        vec![mapping(RANK, &[3, 1, 0], &[1, 2, 3])],
    );
    let source = held(vec![
        unit(PRIABONIAN, "Priabonian"),
        unit(BARTONIAN, "Bartonian"),
        unit(LUTETIAN, "Lutetian"),
        unit(YPRESIAN, "Ypresian"),
        boxed(the_rank),
    ]);
    let set = graphics(&[(PRIABONIAN, RED), (LUTETIAN, BLUE)]);
    let known = Known {
        graphics: &set,
        units: &[],
    };
    let strata = well_strata::of_frame(&well, &Wanted::IntervalUnits { rank: None }, &source, &known)
        .expect("it reads");
    assert_eq!(
        units_of(&strata),
        [Some("Ypresian"), Some("Bartonian"), Some("Priabonian")]
    );

    let grey = Rgba {
        r: 0.5,
        g: 0.5,
        b: 0.5,
        a: 1.0,
    };
    let palette = [GREEN, grey];
    assert_eq!(
        strata.drawn(&palette),
        [
            Drawn::File(RED),
            Drawn::Fallback(GREEN),
            Drawn::File(BLUE),
            Drawn::Fallback(grey)
        ],
        "the file's colour where it gives one, the palette in the rank's order elsewhere"
    );
    // The unit's own colour stays `None`: the palette is how it is drawn, not
    // what the file says.
    assert_eq!(strata.units[1].colour, None);
    let hex = |bytes: Vec<u8>| -> Vec<String> {
        bytes
            .chunks(4)
            .map(|c| format!("#{:02x}{:02x}{:02x}{:02x}", c[0], c[1], c[2], c[3]))
            .collect()
    };
    assert_eq!(
        hex(strata.node_colours_with(GONE, &palette)),
        [grey.to_hex(), GREEN.to_hex(), RED.to_hex(), RED.to_hex()]
    );
    // No palette: what it did before - the colour of absent values.
    assert_eq!(hex(strata.node_colours_with(GONE, &[])), node_hex(&strata, GONE));
    assert_eq!(node_hex(&strata, GONE)[..2], ["#00000000", "#00000000"]);
    assert!(
        strata
            .drawn(&[])
            .iter()
            .filter(|drawn| **drawn == Drawn::Nothing)
            .count()
            == 2
    );

    // Two uncoloured units and one colour: they share it, and that is said.
    assert!(!strata.fallback_cycled(&palette));
    assert!(strata.fallback_cycled(&[GREEN]));
    assert_eq!(strata.drawn(&[GREEN])[3], Drawn::Fallback(GREEN));
    assert!(!strata.fallback_cycled(&[]), "no palette cycles nothing");
}

/// Several `GraphicalInformationSet`s are **read as one** (`GraphicalSet::merged`,
/// one cascade, ADR-0009): the most
/// specific level wins, and at one level the later statement, the package's
/// order being the order of the statements. Two sets disagreeing at that level
/// are **said** and the colour is kept - they used to leave the unit with
/// none, and a recoloured unit went black in the legend.
///
/// What would make it fail: reading only the first set (Bartonian is coloured
/// in the second only), letting the first set that answers win (the first
/// colours the units' **feature**, the second Bartonian itself), or dropping
/// Lutetian's colour because two sets give it two.
#[test]
fn every_set_is_read_and_two_that_disagree_give_the_later_statement_and_say_so() {
    let the_rank = rank(RANK, &[PRIABONIAN, BARTONIAN, LUTETIAN]);
    let well = frame(
        &[0.0, 10.0, 20.0, 30.0],
        vec![mapping(RANK, &[0, 1, 2], &[1, 2, 3])],
    );
    let source = held(vec![
        unit(PRIABONIAN, "Priabonian"),
        unit(BARTONIAN, "Bartonian"),
        unit(LUTETIAN, "Lutetian"),
        boxed(the_rank),
    ]);
    let sets = vec![
        (SET.to_string(), one_set(&[(FEATURE, GREEN), (LUTETIAN, RED)])),
        (
            CHART_SET.to_string(),
            one_set(&[(BARTONIAN, BLUE), (LUTETIAN, BLUE)]),
        ),
    ];
    let known = Known {
        graphics: &sets,
        units: &[],
    };
    let strata = well_strata::of_frame(&well, &Wanted::IntervalUnits { rank: None }, &source, &known)
        .expect("it reads");

    // Priabonian has no entry of its own: its feature's, from the first set.
    assert_eq!(strata.units[0].colour, Some(GREEN));
    assert_eq!(strata.units[0].colour_from.as_deref(), Some(FEATURE));
    // Bartonian's own entry, in the second set, beats its feature's in the first.
    assert_eq!(strata.units[1].colour, Some(BLUE));
    assert_eq!(strata.units[1].colour_from.as_deref(), Some(BARTONIAN));
    // Lutetian: red in one set, blue in the other, both on the unit itself - the
    // later statement, the second set's, is the colour, and the other is said.
    assert_eq!(strata.units[2].colour, Some(BLUE));
    assert_eq!(strata.units[2].colour_from.as_deref(), Some(LUTETIAN));
    assert!(
        strata
            .notes
            .iter()
            .any(|note| note.contains(LUTETIAN) && note.contains(SET) && note.contains("later statement")),
        "the disagreement names the set that is not taken: {:?}",
        strata.notes
    );

    // The package's order is the order of the statements: swapped, the other
    // set is the later one - and the answer is the one the colour column gives.
    let swapped = vec![sets[1].clone(), sets[0].clone()];
    let known = Known {
        graphics: &swapped,
        units: &[],
    };
    let again = well_strata::of_frame(&well, &Wanted::IntervalUnits { rank: None }, &source, &known)
        .expect("it reads");
    assert_eq!(again.units[2].colour, Some(RED));
    assert!(
        again.notes.iter().any(|note| note.contains(LUTETIAN)
            && note.contains(CHART_SET)
            && note.contains("later statement")),
        "{:?}",
        again.notes
    );
}

/// RESQML 2.0.1: the frame spells its field `IntervalStratigraphiUnits` (the
/// XSD's own typo), the mapping names a `StratigraphicOrganization`, the
/// indices are one integer per interval, and the rank's units carry a declared
/// `Index` - which the index designates, whatever their order in the list.
#[test]
fn a_2_0_1_frame_indexes_its_units_by_their_declared_index() {
    let dor20 = |uuid: &str, kind: &str, title: &str| energyml_eml_2_0::DataObjectReference {
        content_type: format!("application/x-resqml+xml;version=2.0;type=obj_{kind}"),
        title: title.to_string(),
        uuid: uuid.to_string(),
        ..Default::default()
    };
    let the_rank = resqml20::StratigraphicColumnRankInterpretation {
        uuid: RANK.to_string(),
        citation: energyml_eml_2_0::Citation {
            title: "formations".to_string(),
            ..Default::default()
        },
        ordering_criteria: resqml20::OrderingCriteria::Age,
        index: 0,
        // Listed 5 then 3: a positional reading finds nothing at 3.
        stratigraphic_units: vec![
            resqml20::StratigraphicUnitInterpretationIndex {
                index: 5,
                unit: dor20(PRIABONIAN, "StratigraphicUnitInterpretation", "Marnes"),
            },
            resqml20::StratigraphicUnitInterpretationIndex {
                index: 3,
                unit: dor20(BARTONIAN, "StratigraphicUnitInterpretation", "Calcaires"),
            },
        ],
        ..Default::default()
    };
    let well = resqml20::WellboreFrameRepresentation {
        uuid: FRAME.to_string(),
        citation: energyml_eml_2_0::Citation {
            title: "F2".to_string(),
            ..Default::default()
        },
        node_count: 3,
        // 100, 110, 120: a lattice, the only inline form 2.0.1 has.
        node_md: Some(resqml20::AbstractDoubleArray::DoubleLatticeArray(
            resqml20::DoubleLatticeArray {
                start_value: 100.0,
                offset: vec![resqml20::DoubleConstantArray {
                    value: 10.0,
                    count: 2,
                }],
            },
        )),
        interval_stratigraphi_units: Some(resqml20::IntervalStratigraphicUnits {
            unit_indices: Some(resqml20::AbstractIntegerArray::IntegerConstantArray(
                resqml20::IntegerConstantArray { value: 3, count: 2 },
            )),
            stratigraphic_organization: dor20(RANK, "StratigraphicColumnRankInterpretation", "formations"),
        }),
        trajectory: dor20(TRAJECTORY, "WellboreTrajectoryRepresentation", "F2"),
        ..Default::default()
    };
    let source = held(vec![boxed(the_rank), boxed(well.clone())]);

    let strata = well_strata::of_frame(
        &well,
        &Wanted::IntervalUnits { rank: None },
        &source,
        &Known::default(),
    )
    .expect("the 2.0.1 spelling is read");

    assert_eq!(units_of(&strata), [Some("Calcaires"), Some("Calcaires")]);
    assert_eq!(strata.sections[1].top, 110.0);
    assert_eq!(strata.sections[1].base, 120.0);
    // The units are not in this package: the rank's cached titles are all there
    // is, and the reader says so rather than presenting them as the units'.
    assert!(
        strata
            .notes
            .iter()
            .any(|note| note.contains("not in the package")),
        "{:?}",
        strata.notes
    );
}

/// *"Since a single interval may corresponds to several units, the mapping is
/// done using a jagged array"* (TRG 2.2 §5.34). Several units for one interval
/// are listed, not arbitrated.
#[test]
fn an_interval_naming_several_units_is_listed_and_drawn_as_none() {
    let the_rank = rank(RANK, &[PRIABONIAN, BARTONIAN]);
    let well = frame(&[0.0, 10.0, 20.0], vec![mapping(RANK, &[0, 1, 1], &[2, 3])]);
    let source = held(vec![
        unit(PRIABONIAN, "Priabonian"),
        unit(BARTONIAN, "Bartonian"),
        boxed(the_rank),
    ]);
    let set = graphics(&[(PRIABONIAN, RED), (BARTONIAN, BLUE)]);
    let known = Known {
        graphics: &set,
        units: &[],
    };

    let strata = well_strata::of_frame(&well, &Wanted::IntervalUnits { rank: None }, &source, &known)
        .expect("it reads");
    assert_eq!(strata.sections[0].unit, None, "two units, and neither is chosen");
    assert_eq!(strata.sections[0].several, [0, 1]);
    assert_eq!(strata.sections[1].unit, Some(1));
    assert!(
        strata.notes.iter().any(|note| note.contains("several units")),
        "{:?}",
        strata.notes
    );
    assert_eq!(node_hex(&strata, GONE), ["#00000000", "#0000ffff", "#0000ffff"]);
}

// -- a category property --------------------------------------------------------

/// A category table keyed `10, 20, 35`, and a property naming its rows by key.
fn category_package(
    values: &[i64],
) -> (
    resqml22::WellboreFrameRepresentation,
    resqml22::DiscreteProperty,
    Prefetched,
) {
    let well = frame(&[0.0, 5.0, 12.0, 20.0, 31.0], Vec::new());
    let table = eml23::ColumnBasedTable {
        uuid: TABLE.to_string(),
        citation: citation("STRA"),
        // Written **after** the value column, as the reference package writes
        // its key: the key is the column the document calls a key, not the
        // first one.
        column: vec![eml23::Column {
            title: Some("Nom_STRA".to_string()),
            value_count_per_row: 1,
            values: Some(eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
                count_per_value: 1,
                values: vec!["Lutetian".into(), "Bartonian".into(), "Priabonian".into()],
                ..Default::default()
            })),
            ..Default::default()
        }],
        key_column: vec![eml23::Column {
            title: Some("STRA".to_string()),
            value_count_per_row: 1,
            values: Some(eml23::AbstractValueArray::IntegerXmlArray(integers(&[
                10, 20, 35,
            ]))),
            ..Default::default()
        }],
        ..Default::default()
    };
    let property = resqml22::DiscreteProperty {
        uuid: PROPERTY.to_string(),
        citation: citation("F1 - STRA"),
        indexable_element: eml23::IndexableElement::Intervals,
        value_count_per_indexable_element: vec![1],
        supporting_representation: dor(FRAME, "resqml22.WellboreFrameRepresentation", "F1 - frame"),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerXmlArray(integers(values))],
        category_lookup: Some(dor(TABLE, "eml23.ColumnBasedTable", "STRA")),
        ..Default::default()
    };
    let source = held(vec![boxed(well.clone()), boxed(table), boxed(property.clone())]);
    (well, property, source)
}

/// Common TRG 2.3 §9.1: *"Defines a key column which make the table a lookup
/// table."* The value is looked up in the key column - the keys here are not
/// `0..n`, so a position lookup would find no row at all.
#[test]
fn a_category_is_looked_up_by_its_key_not_by_its_position() {
    let (well, property, source) = category_package(&[20, 10, 35, -1]);
    // Two units of the package are titled Lutetian: the row does not choose.
    let units = [
        (BARTONIAN.to_string(), "Bartonian".to_string()),
        (LUTETIAN.to_string(), "Lutetian".to_string()),
        (YPRESIAN.to_string(), "Lutetian".to_string()),
    ];
    let set = graphics(&[(BARTONIAN, GREEN)]);
    let known = Known {
        graphics: &set,
        units: &units,
    };

    let strata = well_strata::of_frame(&well, &Wanted::Category { property: &property }, &source, &known)
        .expect("the property names its rows");

    assert_eq!(
        strata.source,
        Source::Category {
            property: PROPERTY.to_string(),
            table: TABLE.to_string()
        }
    );
    assert_eq!(
        units_of(&strata),
        [Some("Bartonian"), Some("Lutetian"), Some("Priabonian"), None]
    );
    // One unit per row, keyed on the key - what the front end's category legend
    // keys on too.
    assert_eq!(
        strata.units.iter().map(|unit| unit.code).collect::<Vec<_>>(),
        [10.0, 20.0, 35.0]
    );

    let bartonian = &strata.units[1];
    assert_eq!(bartonian.uuid.as_deref(), Some(BARTONIAN));
    assert!(bartonian.by_title, "found by its title, and said so");
    assert_eq!(bartonian.colour.map(Rgba::to_hex), Some("#00ff00ff".to_string()));

    let lutetian = &strata.units[0];
    assert_eq!(lutetian.uuid, None, "two units carry that title");
    assert!(
        strata.notes.iter().any(|note| note.contains("matches 2 units")),
        "{:?}",
        strata.notes
    );
}

#[test]
fn a_code_no_row_holds_is_refused_by_name() {
    let (well, property, source) = category_package(&[20, 15, 35, 10]);
    let refused = well_strata::of_frame(
        &well,
        &Wanted::Category { property: &property },
        &source,
        &Known::default(),
    )
    .expect_err("15 names no row");
    assert!(matches!(refused, Refusal::Defect { .. }), "{refused:?}");
    let said = refused.to_string();
    assert!(
        said.contains("stores 15") && said.contains("interval 1"),
        "{said}"
    );
}

// -- refusals -----------------------------------------------------------------

#[test]
fn a_frame_that_maps_no_unit_is_an_absence_not_a_fault() {
    let well = frame(&[0.0, 10.0], Vec::new());
    let source = held(vec![boxed(well.clone())]);
    let refused = well_strata::of_frame(
        &well,
        &Wanted::IntervalUnits { rank: None },
        &source,
        &Known::default(),
    )
    .expect_err("nothing to read");
    assert!(refused.is_absence(), "{refused:?}");
    assert!(
        refused.to_string().contains("IntervalStratigraphicUnits"),
        "{refused}"
    );
}

#[test]
fn a_rank_the_package_does_not_hold_is_named() {
    let well = frame(&[0.0, 10.0], vec![mapping(RANK, &[0], &[1])]);
    let source = held(vec![boxed(well.clone())]);
    let refused = well_strata::of_frame(
        &well,
        &Wanted::IntervalUnits { rank: None },
        &source,
        &Known::default(),
    )
    .expect_err("the rank is missing");
    assert!(refused.is_absence(), "{refused:?}");
    assert!(refused.to_string().contains(RANK), "{refused}");
}

#[test]
fn an_index_outside_its_rank_is_a_fault_of_the_file() {
    let the_rank = rank(RANK, &[PRIABONIAN, BARTONIAN, LUTETIAN]);
    let well = frame(&[0.0, 10.0, 20.0], vec![mapping(RANK, &[0, 7], &[1, 2])]);
    let source = held(vec![boxed(the_rank), boxed(well.clone())]);
    let refused = well_strata::of_frame(
        &well,
        &Wanted::IntervalUnits { rank: None },
        &source,
        &Known::default(),
    )
    .expect_err("7 is not in the rank");
    assert!(matches!(refused, Refusal::Defect { .. }), "{refused:?}");
    let said = refused.to_string();
    assert!(said.contains("unit 7") && said.contains("interval 1"), "{said}");
}

/// TRG 2.2 §5.34: *"BUSINESS RULE: Array length must equal the number of
/// intervals."* One run short shifts every section, so it is refused.
#[test]
fn a_mapping_one_interval_short_is_refused() {
    let the_rank = rank(RANK, &[PRIABONIAN]);
    let well = frame(&[0.0, 10.0, 20.0, 30.0], vec![mapping(RANK, &[0, 0], &[1, 2])]);
    let source = held(vec![boxed(the_rank), boxed(well.clone())]);
    let refused = well_strata::of_frame(
        &well,
        &Wanted::IntervalUnits { rank: None },
        &source,
        &Known::default(),
    )
    .expect_err("two runs for three intervals");
    assert!(matches!(refused, Refusal::Defect { .. }), "{refused:?}");
    assert!(refused.to_string().contains("§5.34"), "{refused}");
}

/// Two ranks naming two different units for one interval: the TRG ranks no
/// rank above another, so the reader does not choose - and a named rank does.
#[test]
fn two_ranks_that_disagree_are_ambiguous_until_one_is_named() {
    let stages = rank(RANK, &[BARTONIAN]);
    let series = rank(OTHER_RANK, &[YPRESIAN]);
    let well = frame(
        &[0.0, 10.0, 20.0],
        vec![
            mapping(RANK, &[0, -1], &[1, 2]),
            mapping(OTHER_RANK, &[0, 0], &[1, 2]),
        ],
    );
    let source = held(vec![
        unit(BARTONIAN, "Bartonian"),
        unit(YPRESIAN, "Eocene"),
        boxed(stages),
        boxed(series),
        boxed(well.clone()),
    ]);

    let refused = well_strata::of_frame(
        &well,
        &Wanted::IntervalUnits { rank: None },
        &source,
        &Known::default(),
    )
    .expect_err("interval 0 is given two units");
    assert!(matches!(refused, Refusal::Ambiguous { .. }), "{refused:?}");
    let said = refused.to_string();
    assert!(
        said.contains("Bartonian") && said.contains("Eocene") && said.contains("interval 0"),
        "{said}"
    );

    let chosen = well_strata::of_frame(
        &well,
        &Wanted::IntervalUnits {
            rank: Some(&OTHER_RANK.to_uppercase()),
        },
        &source,
        &Known::default(),
    )
    .expect("one rank named");
    assert_eq!(units_of(&chosen), [Some("Eocene"), Some("Eocene")]);
    assert_eq!(
        chosen.source,
        Source::IntervalUnits {
            ranks: vec![OTHER_RANK.to_string()]
        }
    );

    // Where they do **not** disagree - a mapping per rank, each interval named
    // by one of them - the two merge, which is how a package written by
    // `wells-and-properties-from-table` maps its passes.
    let merged = frame(
        &[0.0, 10.0, 20.0],
        vec![
            mapping(RANK, &[-1, 0], &[1, 2]),
            mapping(OTHER_RANK, &[0, -1], &[1, 2]),
        ],
    );
    let strata = well_strata::of_frame(
        &merged,
        &Wanted::IntervalUnits { rank: None },
        &source,
        &Known::default(),
    )
    .expect("no interval is given two units");
    assert_eq!(units_of(&strata), [Some("Eocene"), Some("Bartonian")]);
}

/// TRG 2.2 §5.34 lets the mapping name any
/// `AbstractGeologicUnitOrganizationInterpretation`. An occurrence (§6.31)
/// holds a list of units of its own **and** names the rank it is an
/// occurrence of; which of the two an index counts in is not written anywhere.
#[test]
fn an_occurrence_is_refused_as_the_choice_it_leaves_open() {
    let occurrence = resqml22::GeologicUnitOccurrenceInterpretation {
        uuid: RANK.to_string(),
        citation: citation("along F1"),
        geologic_unit: vec![dor(
            BARTONIAN,
            "resqml22.StratigraphicUnitInterpretation",
            "Bartonian",
        )],
        is_occurrence_of: Some(dor(
            OTHER_RANK,
            "resqml22.StratigraphicColumnRankInterpretation",
            "stages",
        )),
        ..Default::default()
    };
    let well = frame(&[0.0, 10.0], vec![mapping(RANK, &[0], &[1])]);
    let source = held(vec![boxed(occurrence), boxed(well.clone())]);
    let refused = well_strata::of_frame(
        &well,
        &Wanted::IntervalUnits { rank: None },
        &source,
        &Known::default(),
    )
    .expect_err("two lists, and nothing says which");
    assert!(matches!(refused, Refusal::Ambiguous { .. }), "{refused:?}");
    assert!(refused.to_string().contains("IsOccurrenceOf"), "{refused}");
}

#[test]
fn a_witsml_wellbore_is_refused_by_name_rather_than_drawn() {
    let geology = energyml_witsml_2_1::WellboreGeology::default();
    let refused = well_strata::of_frame(
        &geology,
        &Wanted::IntervalUnits { rank: None },
        &held(Vec::new()),
        &Known::default(),
    )
    .expect_err("no WITSML wellbore is drawn");
    assert!(matches!(refused, Refusal::Unsupported { .. }), "{refused:?}");
    assert!(refused.to_string().contains("WITSML"), "{refused}");
}

#[test]
fn what_is_not_a_frame_is_refused_by_name() {
    let surface = resqml22::TriangulatedSetRepresentation::default();
    let refused = well_strata::of_frame(
        &surface,
        &Wanted::IntervalUnits { rank: None },
        &held(Vec::new()),
        &Known::default(),
    )
    .expect_err("a surface has no MD");
    assert!(matches!(refused, Refusal::Unsupported { .. }), "{refused:?}");
    assert!(refused.to_string().contains("NodeMd"), "{refused}");
}
