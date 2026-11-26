//! The unit of a hue, read from the numbers of the colours it belongs to (the rule is `energyml_core::color::hue_unit`).
//!
//! RESQML TRG 2.2 section 4.21 (`HsvColor`) gives `Hue` no unit. A map whose hues all lie in 0-1 with one strictly between is
//! read in turns, a map of degrees is not touched, and the reading in turns is **said** in a note on the map - and never
//! warned of by the validation: turns are a valid form. Every object is built here.
//!
//! What it would take to make each one fail: deciding on one colour alone, leaving out the null, above and below colours, taking 0
//! and 1 as turns, forgetting the note, or reporting turns as a finding of the validation.

use energyml_core::any_xml::AnyXml;
use energyml_core::registry::DynObject;
use energyml_core::validate;
use energyml_domain::check;
use energyml_domain::color::{ColorMap, GraphicalSet, HUE_IN_TURNS, Information};
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_2 as resqml22;

fn hsv(hue: f64) -> resqml22::HsvColor {
    resqml22::HsvColor {
        alpha: 1.0,
        hue,
        saturation: 1.0,
        value: 1.0,
        title: None,
    }
}

fn discrete(hues: &[f64]) -> resqml22::DiscreteColorMap {
    resqml22::DiscreteColorMap {
        uuid: "e0000000-0000-4000-8000-000000000001".to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: "colours".to_string(),
            ..Default::default()
        },
        entry: hues
            .iter()
            .enumerate()
            .map(|(index, hue)| resqml22::DiscreteColorMapEntry {
                index: index as i64,
                hsv: hsv(*hue),
            })
            .collect(),
        ..Default::default()
    }
}

fn hex_of(map: &ColorMap, key: f64) -> String {
    map.sample_key(key).expect("the key has its entry").to_hex()
}

/// The map of a package that writes fractions of a turn: the five hues of its strata (0.602, 0.167, 0.922, 0.916 and 0), in
/// 0-1 with four strictly between, are turns - the first (0.602 turn, 217 degrees) comes out blue and the second
/// (0.167 turn, 60 degrees) yellow, and the map says it read them so.
///
/// Break it: in `settle_hues`, return `None` for every set (hues are then degrees: the first entry is a red).
#[test]
fn a_map_of_hues_in_zero_to_one_is_read_in_turns_and_says_so() {
    let (map, notes) = ColorMap::read_noted(&discrete(&[
        0.602_409_66,
        0.166_666_67,
        0.922_338_5,
        0.916_010_5,
        0.0,
    ]))
    .expect("the map reads");
    let blue = map.sample_key(0.0).expect("Q").to_bytes();
    assert!(blue[2] > blue[1] && blue[1] > blue[0], "a blue: {blue:?}");
    assert_eq!(hex_of(&map, 1.0), "#ffff00ff", "yellow");
    // The hue is stored in degrees whatever the file wrote.
    assert!(
        (map.stops[1].color.hue - 60.0).abs() < 1e-4,
        "{}",
        map.stops[1].color.hue
    );
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].contains(HUE_IN_TURNS)
            && notes[0].contains("all 5 hues lie in 0-1")
            && notes[0].contains("e0000000"),
        "{}",
        notes[0]
    );
}

/// A map in degrees is not touched, and says nothing; so is a map with a hue above 1 among hues below it (a hue in `]1, 360]`
/// forces degrees for the whole set).
///
/// Break it: in `hue_unit`, decide on `any` hue within 0-1 and not on `all`.
#[test]
fn a_map_in_degrees_is_unchanged_and_silent() {
    let (map, notes) = ColorMap::read_noted(&discrete(&[120.0, 240.0])).expect("the map reads");
    assert_eq!(hex_of(&map, 0.0), "#00ff00ff");
    assert_eq!(hex_of(&map, 1.0), "#0000ffff");
    assert!(notes.is_empty(), "{notes:?}");

    let (map, notes) = ColorMap::read_noted(&discrete(&[0.5, 120.0])).expect("the map reads");
    assert!(
        (map.stops[0].color.hue - 0.5).abs() < 1e-12,
        "0.5 degree: a hue above 1 sets the unit of the whole map"
    );
    assert!(notes.is_empty(), "{notes:?}");
}

/// A map whose only hues are 0 and 1 stays in degrees: 0 and 1 turn are the red of 0 and 1 degree, to a degree, so they
/// decide nothing, and there is nothing to say.
///
/// Break it: in `hue_unit`, count a hue of exactly 1 as strictly between 0 and 1.
#[test]
fn a_map_of_zero_and_one_only_stays_in_degrees() {
    let (map, notes) = ColorMap::read_noted(&discrete(&[0.0, 1.0])).expect("the map reads");
    assert_eq!(map.stops[1].color.hue, 1.0, "one degree, not a turn");
    assert!(notes.is_empty(), "{notes:?}");
}

/// The unit is the **map's**, not each colour's: entries at 0 and 1 are decided by the null colour, which lies between
/// (so all three are turns), and a hue of 200 among the others makes the map degrees even where an entry says 0.5.
///
/// Break it: in `ColorMap::read_noted`, leave the null, above and below colours out of the set the unit is decided on.
#[test]
fn the_null_above_and_below_colours_are_part_of_the_map() {
    let mut continuous = resqml22::ContinuousColorMap {
        uuid: "e0000000-0000-4000-8000-000000000002".to_string(),
        schema_version: "2.2".into(),
        citation: eml23::Citation {
            title: "continuous".to_string(),
            ..Default::default()
        },
        entry: [0.0, 1.0]
            .iter()
            .map(|index| resqml22::ContinuousColorMapEntry {
                index: *index,
                hsv: hsv(*index),
            })
            .collect(),
        null_color: Some(hsv(0.5)),
        ..Default::default()
    };
    let (map, notes) = ColorMap::read_noted(&continuous).expect("the map reads");
    assert_eq!(
        notes.len(),
        1,
        "the null colour decides for the entries: {notes:?}"
    );
    assert!(notes[0].contains("all 3 hues"), "{}", notes[0]);
    assert_eq!(map.stops[1].color.hue, 360.0);
    assert_eq!(map.null_color.expect("null").hue, 180.0);

    continuous.below_min_color = Some(hsv(200.0));
    let (map, notes) = ColorMap::read_noted(&continuous).expect("the map reads");
    assert!(
        notes.is_empty(),
        "a hue of 200 in the below colour: degrees for the map"
    );
    assert_eq!(map.stops[1].color.hue, 1.0);
}

/// An entry of a hue that is no hue is refused by name, as it always was: the rule decides a unit, never a bound.
///
/// Break it: in `Hsv::checked`, drop the bound on the hue.
#[test]
fn a_hue_outside_zero_to_360_is_still_refused() {
    let refused = ColorMap::read_noted(&discrete(&[0.5, 400.0])).expect_err("refused");
    assert!(refused.to_string().contains("Hue"), "{refused}");
}

fn set_of(hues: &[f64]) -> eml23::GraphicalInformationSet {
    let entry = |hue: f64| AnyXml {
        attributes: vec![(
            "xsi:type".to_string(),
            "resqml22:DefaultGraphicalInformation".to_string(),
        )],
        content: format!(
            "<eml23:TargetObject><eml23:Uuid>e0000000-0000-4000-8000-0000000000a1</eml23:Uuid>\
             <eml23:QualifiedType>resqml22.WellboreTrajectoryRepresentation</eml23:QualifiedType>\
             <eml23:Title>t</eml23:Title></eml23:TargetObject>\
             <resqml22:ViewerKind>3d</resqml22:ViewerKind>\
             <resqml22:IndexableElementInfo xsi:type=\"resqml22:GraphicalInformationForWholeObject\">\
             <resqml22:IsVisible>true</resqml22:IsVisible>\
             <resqml22:ConstantColor><resqml22:Alpha>1</resqml22:Alpha><resqml22:Hue>{hue}</resqml22:Hue>\
             <resqml22:Saturation>1</resqml22:Saturation><resqml22:Value>1</resqml22:Value></resqml22:ConstantColor>\
             </resqml22:IndexableElementInfo>"
        ),
        namespaces: Vec::new(),
    };
    eml23::GraphicalInformationSet {
        uuid: "e0000000-0000-4000-8000-0000000000f1".to_string(),
        schema_version: "2.3".into(),
        citation: eml23::Citation {
            title: "a set".to_string(),
            ..Default::default()
        },
        graphical_information: hues.iter().map(|hue| entry(*hue)).collect(),
        ..Default::default()
    }
}

fn constant_hues(set: &GraphicalSet) -> Vec<f64> {
    set.entries
        .iter()
        .filter_map(|entry| match &entry.information {
            Information::Default { elements, .. } => elements.first()?.constant_color.as_ref().map(|c| c.hue),
            _ => None,
        })
        .collect()
}

/// A constant colour alone decides nothing: its set does. Two entries at 0.5 and 0 are turns (a hue strictly between), the
/// same 0.5 beside a 120 is half a degree; and the set says it read turns.
///
/// Break it: in `GraphicalSet::read`, decide each constant colour on its own.
#[test]
fn a_constant_colour_is_decided_by_its_set() {
    let turns = GraphicalSet::read(&set_of(&[0.5, 0.0]));
    assert_eq!(constant_hues(&turns), [180.0, 0.0]);
    assert!(turns.hue_in_turns(), "{:?}", turns.notes);
    assert!(
        turns
            .notes
            .iter()
            .any(|note| note.contains("all 2 hues lie in 0-1")),
        "{:?}",
        turns.notes
    );

    let degrees = GraphicalSet::read(&set_of(&[0.5, 120.0]));
    assert_eq!(constant_hues(&degrees), [0.5, 120.0]);
    assert!(!degrees.hue_in_turns());

    let undecided = GraphicalSet::read(&set_of(&[0.0, 1.0]));
    assert_eq!(constant_hues(&undecided), [0.0, 1.0]);
    assert!(!undecided.hue_in_turns());
}

/// The validation says **nothing** of hues in turns: a map in fractions of a turn is a valid form (the schema fixes no unit, and
/// the translator writes this one), so a package that has them leaves `energyml check` with no finding of any kind about its
/// colours - while the map keeps its note, which says how it was read.
///
/// Break it: add a rule to `documented_rules()` that reports a map read in turns.
#[test]
fn a_map_in_turns_is_not_a_finding_of_the_validation_and_keeps_its_note() {
    let rules = check::documented_rules();
    let judge = |object: Box<dyn DynObject>| validate::validate_with(object.as_validate(), &rules).issues;
    // The five hues of a strata map written in fractions of a turn.
    let in_turns = discrete(&[0.602_409_66, 0.166_666_67, 0.922_338_5, 0.916_010_5, 0.0]);
    let issues = judge(Box::new(in_turns.clone()));
    assert!(
        issues.iter().all(|issue| !issue.code.starts_with("colour")),
        "no finding about the colours: {issues:?}"
    );
    let set = judge(Box::new(set_of(&[0.5, 0.0])));
    assert!(
        set.iter().all(|issue| !issue.code.starts_with("colour")),
        "{set:?}"
    );

    let (_, notes) = ColorMap::read_noted(&in_turns).expect("the map reads");
    assert!(
        notes.iter().any(|note| note.contains(HUE_IN_TURNS)),
        "the note says how it was read: {notes:?}"
    );
}
