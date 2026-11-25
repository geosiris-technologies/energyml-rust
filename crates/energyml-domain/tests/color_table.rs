//! The colour a `ColorInformation` gives **the table of a category property**
//! (issue #35 of the public repository, ADR-0050).
//!
//! The form comes from the schemas and the guides, not from a package: a
//! `ColorInformation` names its `TargetObject`s and a `ColorMap`
//! (`AbstractGraphicalInformation`, RESQML TRG 2.2 §13); a `DiscreteColorMap`
//! entry carries an integer `index` and an `HsvColor` (§13.2); the table is a
//! `ColumnBasedTable` (common TRG 2.3 §9.1: a key column makes it a lookup, the
//! columns follow the keys) in 2.2 and a `StringTableLookup` of key/value pairs in
//! 2.0.1. The map's entries are indexed by the table's **keys**, and
//! `ValueVectorIndex` designates a **column** of the table, counting the key
//! column from 0 - the one reading that makes the known packages say something.
//! Every object is built here; nothing is read from a file.
//!
//! # What it would take to make each one fail
//!
//! - never asking the table (the level absent): the first test finds nothing;
//! - reading an index the table does not hold as a column anyway, or a table keyed
//!   on two columns as if one key named a row;
//! - sampling a key that has no entry with the nearest lower entry's colour;
//! - putting the table above the property's own colour, or below its kind's.

use energyml_core::registry::DynObject;
use energyml_core::value::Reflect;
use energyml_domain::color::{self, Cascade, Decorated, GraphicalSet, Level, Paint, Request, Scaling, Scope};
use energyml_domain::source::Prefetched;
use energyml_domain::well_strata::{self, Known, Wanted};
use energyml_eml_2_0 as eml20;
use energyml_eml_2_3 as eml23;
use energyml_resqml_2_0_1 as resqml20;
use energyml_resqml_2_2 as resqml22;
use energyml_storage::array::ArrayLocation;

const FRAME: &str = "f0000000-0000-4000-8000-000000000001";
const TRAJECTORY: &str = "f0000000-0000-4000-8000-000000000002";
const PROPERTY: &str = "c0000000-0000-4000-8000-000000000001";
const TABLE: &str = "c0000000-0000-4000-8000-000000000002";
const KIND: &str = "c0000000-0000-4000-8000-000000000003";
const MAP_OF_TABLE: &str = "d0000000-0000-4000-8000-000000000001";
const MAP_OF_KIND: &str = "d0000000-0000-4000-8000-000000000002";
const MAP_OF_PROPERTY: &str = "d0000000-0000-4000-8000-000000000003";

// -- building blocks ---------------------------------------------------------

fn nowhere(_: &dyn Reflect) -> Vec<ArrayLocation> {
    Vec::new()
}

fn held(objects: Vec<Box<dyn DynObject>>) -> Prefetched {
    let mut cache = Prefetched::new();
    for object in objects {
        cache.insert(object, &nowhere);
    }
    cache
}

fn boxed<T: DynObject + 'static>(object: T) -> Box<dyn DynObject> {
    Box::new(object)
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

fn column(title: &str, values: eml23::AbstractValueArray) -> eml23::Column {
    eml23::Column {
        title: Some(title.to_string()),
        value_count_per_row: 1,
        values: Some(values),
        ..Default::default()
    }
}

/// A 2.2 table keyed `10, 20, 35`, with its labels: key column first.
fn table_22(key_columns: usize) -> eml23::ColumnBasedTable {
    let key = |title: &str| {
        column(
            title,
            eml23::AbstractValueArray::IntegerXmlArray(integers(&[10, 20, 35])),
        )
    };
    eml23::ColumnBasedTable {
        uuid: TABLE.to_string(),
        citation: citation("STRA"),
        key_column: (0..key_columns).map(|at| key(&format!("key {at}"))).collect(),
        column: vec![column(
            "Nom_STRA",
            eml23::AbstractValueArray::StringXmlArray(eml23::StringXmlArray {
                count_per_value: 1,
                values: vec!["Lutetian".into(), "Bartonian".into(), "Priabonian".into()],
                ..Default::default()
            }),
        )],
        ..Default::default()
    }
}

/// The 2.0.1 form of the same table: pairs, key then value.
fn table_201() -> resqml20::StringTableLookup {
    let pair = |key: i64, value: &str| resqml20::StringLookup {
        key,
        value: value.to_string(),
    };
    resqml20::StringTableLookup {
        uuid: TABLE.to_string(),
        schema_version: "2.0".to_string(),
        citation: eml20::Citation {
            title: "STRA".to_string(),
            ..Default::default()
        },
        value: vec![
            pair(10, "Lutetian"),
            pair(20, "Bartonian"),
            pair(35, "Priabonian"),
        ],
        ..Default::default()
    }
}

fn hsv(hue: f64) -> resqml22::HsvColor {
    resqml22::HsvColor {
        alpha: 1.0,
        hue,
        saturation: 1.0,
        value: 1.0,
        title: None,
    }
}

/// A discrete map: key 10 red, 20 green, 35 blue - or without the last entry.
fn map(uuid: &str, with_35: bool) -> resqml22::DiscreteColorMap {
    let entry = |index: i64, hue: f64| resqml22::DiscreteColorMapEntry { index, hsv: hsv(hue) };
    let mut entries = vec![entry(10, 0.0), entry(20, 120.0)];
    if with_35 {
        entries.push(entry(35, 240.0));
    }
    resqml22::DiscreteColorMap {
        uuid: uuid.to_string(),
        citation: citation("colours"),
        entry: entries,
        ..Default::default()
    }
}

fn decorated(uuid: &str, qualified_type: &str) -> Decorated {
    Decorated {
        uuid: uuid.to_string(),
        qualified_type: qualified_type.to_string(),
        title: "x".to_string(),
    }
}

/// A set with these colour maps: (target, its type, the map, `ValueVectorIndex`).
fn set_of(entries: &[(&str, &str, &str, Option<u64>)]) -> GraphicalSet {
    let mut set = eml23::GraphicalInformationSet::default();
    for (target, qualified_type, map_uuid, index) in entries {
        color::set_colour_map(
            set.as_reflect_mut(),
            &[decorated(target, qualified_type)],
            &decorated(map_uuid, "resqml22.DiscreteColorMap"),
            &Scaling::default(),
            *index,
        )
        .expect("the entry is written");
    }
    GraphicalSet::read(set.as_reflect())
}

const TABLE_22: &str = "eml23.ColumnBasedTable";
const TABLE_201: &str = "resqml20.obj_StringTableLookup";

fn source(table: Box<dyn DynObject>) -> Prefetched {
    held(vec![
        table,
        boxed(map(MAP_OF_TABLE, true)),
        boxed(map(MAP_OF_KIND, true)),
        boxed(map(MAP_OF_PROPERTY, true)),
    ])
}

// -- the level --------------------------------------------------------------------

/// A `ColorInformation` on the table is read, in both forms of the table; its map
/// is indexed by the keys and `ValueVectorIndex` is the column.
///
/// Break it: in `Cascade::table_decision`, return `Ok(None)` at once.
#[test]
fn a_colour_map_on_the_table_is_read_in_both_forms_of_the_table() {
    for (qualified_type, table) in [(TABLE_22, boxed(table_22(1))), (TABLE_201, boxed(table_201()))] {
        let set = set_of(&[(TABLE, qualified_type, MAP_OF_TABLE, Some(1))]);
        let cascade = Cascade::new(&set);
        let decision = cascade
            .table_decision(TABLE, &source(table))
            .expect("column 1 is the label column of either form")
            .unwrap_or_else(|| panic!("{qualified_type}: nothing found on the table"));
        assert_eq!(decision.level, Level::CategoryTable, "{qualified_type}");
        let Paint::Mapped {
            map,
            value_vector_index,
            ..
        } = &decision.paint
        else {
            panic!("a colour map");
        };
        assert_eq!(*value_vector_index, Some(1), "the column, not a component");
        // The entries are indexed by the keys of the table: 10, 20, 35.
        assert_eq!(
            map.sample_key(20.0).map(|rgba| rgba.to_hex()),
            Some("#00ff00ff".to_string())
        );
        assert_eq!(
            map.sample_key(35.0).map(|rgba| rgba.to_hex()),
            Some("#0000ffff".to_string())
        );
    }
}

/// An index that cannot be placed is refused by its name: a column the table does
/// not have (2 in a two-column table), a table keyed on two columns, a column of
/// a table the package does not hold.
///
/// Break it: in `check_table_column`, delete the `column >= read.columns.len()` test.
#[test]
fn an_index_that_cannot_be_placed_is_refused_by_name() {
    let set = set_of(&[(TABLE, TABLE_22, MAP_OF_TABLE, Some(2))]);
    let refused = Cascade::new(&set)
        .table_decision(TABLE, &source(boxed(table_22(1))))
        .expect_err("the table has columns 0 and 1");
    let said = refused.to_string();
    assert!(
        said.contains("ValueVectorIndex") && said.contains('2') && said.contains("names no column"),
        "{said}"
    );

    let set = set_of(&[(TABLE, TABLE_201, MAP_OF_TABLE, Some(2))]);
    assert!(
        Cascade::new(&set)
            .table_decision(TABLE, &source(boxed(table_201())))
            .is_err(),
        "a 2.0.1 lookup has a key and a value: two columns"
    );

    let set = set_of(&[(TABLE, TABLE_22, MAP_OF_TABLE, Some(1))]);
    let refused = Cascade::new(&set)
        .table_decision(TABLE, &source(boxed(table_22(2))))
        .expect_err("two key columns");
    assert!(refused.to_string().contains("keyed on 2 columns"), "{refused}");

    let refused = Cascade::new(&set)
        .table_decision(TABLE, &held(vec![boxed(map(MAP_OF_TABLE, true))]))
        .expect_err("a table that is not there");
    assert!(refused.to_string().contains("does not hold"), "{refused}");

    // No index at all means the table as a whole: nothing to place, nothing refused.
    let set = set_of(&[(TABLE, TABLE_22, MAP_OF_TABLE, None)]);
    assert!(
        Cascade::new(&set)
            .table_decision(TABLE, &source(boxed(table_22(1))))
            .expect("nothing to place")
            .is_some()
    );
}

/// A key the map has no entry for has **no colour**: the nearest-lower rule of a
/// discrete map would give it another category's.
///
/// Break it: in `ColorMap::sample_key`, take the last stop whose index is at most the key.
#[test]
fn a_key_with_no_entry_has_no_colour() {
    let map = color::ColorMap::read(map(MAP_OF_TABLE, false).as_reflect()).expect("the map reads");
    assert!(map.sample_key(20.0).is_some());
    assert_eq!(map.sample_key(35.0), None, "35 has no entry");
    assert_eq!(map.sample_key(25.0), None, "25 is no key at all");
}

// -- where the level stands in the cascade ------------------------------------------

/// The property's own colour first, then its table, then its kind: the table is
/// about these categories, the kind about every property of that sort.
///
/// Break it: in `Cascade::resolve_category`, ask `self.resolve(request, objects)`
/// before the table.
#[test]
fn the_table_stands_between_the_property_and_its_kind() {
    let kind = [KIND.to_string()];
    let request = Request {
        representation: FRAME,
        property: Some(PROPERTY),
        kind_chain: &kind,
        element: Scope::WholeObject,
    };
    let objects = source(boxed(table_22(1)));
    let on_kind = ("eml23.PropertyKind", KIND, MAP_OF_KIND);

    let set = set_of(&[
        (on_kind.1, on_kind.0, on_kind.2, None),
        (TABLE, TABLE_22, MAP_OF_TABLE, Some(1)),
    ]);
    let cascade = Cascade::new(&set);
    let found = cascade
        .resolve_category(&request, TABLE, &objects)
        .expect("resolves")
        .expect("something answers");
    assert_eq!(found.level, Level::CategoryTable, "the table before the kind");
    let plain = cascade
        .resolve(&request, &objects)
        .expect("resolves")
        .expect("the kind answers");
    assert_eq!(plain.level, Level::PropertyKind, "`resolve` never asks the table");

    let set = set_of(&[
        (on_kind.1, on_kind.0, on_kind.2, None),
        (TABLE, TABLE_22, MAP_OF_TABLE, Some(1)),
        (PROPERTY, "resqml22.DiscreteProperty", MAP_OF_PROPERTY, None),
    ]);
    let found = Cascade::new(&set)
        .resolve_category(&request, TABLE, &objects)
        .expect("resolves")
        .expect("something answers");
    assert_eq!(found.level, Level::Property, "the property's own word first");

    let set = set_of(&[(on_kind.1, on_kind.0, on_kind.2, None)]);
    let found = Cascade::new(&set)
        .resolve_category(&request, TABLE, &objects)
        .expect("resolves")
        .expect("the kind answers");
    assert_eq!(
        found.level,
        Level::PropertyKind,
        "nothing on the table: as before"
    );
}

// -- the units of a well -----------------------------------------------------------

fn frame(mds: &[f64]) -> resqml22::WellboreFrameRepresentation {
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
        ..Default::default()
    }
}

fn discrete(codes: &[i64]) -> resqml22::DiscreteProperty {
    resqml22::DiscreteProperty {
        uuid: PROPERTY.to_string(),
        citation: citation("F1 - STRA"),
        indexable_element: eml23::IndexableElement::Intervals,
        value_count_per_indexable_element: vec![1],
        supporting_representation: dor(FRAME, "resqml22.WellboreFrameRepresentation", "F1 - frame"),
        values_for_patch: vec![resqml22::AbstractValueArray::IntegerXmlArray(integers(codes))],
        category_lookup: Some(dor(TABLE, TABLE_22, "STRA")),
        ..Default::default()
    }
}

/// The categories of a well take the colour of the map on their table - by key,
/// the way the issue's packages say it - in the 3D legend and the column alike
/// (`well_strata`); a key with no entry is left uncoloured and said.
///
/// Break it: in `well_strata::property_map`, ask `cascade.resolve(..)` instead of
/// `cascade.resolve_category(..)`.
#[test]
fn the_categories_of_a_well_take_the_colour_of_the_map_on_their_table() {
    let mds = [0.0, 5.0, 12.0, 20.0];
    let property = discrete(&[20, 10, 35]);
    let well = frame(&mds);
    let titled = |with_35: bool| {
        let objects = held(vec![
            boxed(well.clone()),
            boxed(table_22(1)),
            boxed(property.clone()),
            boxed(map(MAP_OF_TABLE, with_35)),
        ]);
        let set = vec![(
            "d0000000-0000-4000-8000-0000000000ff".to_string(),
            set_of(&[(TABLE, TABLE_22, MAP_OF_TABLE, Some(1))]),
        )];
        let known = Known {
            graphics: &set,
            units: &[],
        };
        well_strata::of_frame(&well, &Wanted::Category { property: &property }, &objects, &known)
            .expect("the property names its rows")
    };

    let strata = titled(true);
    let colours: Vec<(String, Option<String>)> = strata
        .units
        .iter()
        .map(|unit| (unit.title.clone(), unit.colour.map(|rgba| rgba.to_hex())))
        .collect();
    assert_eq!(
        colours,
        [
            ("Lutetian".to_string(), Some("#ff0000ff".to_string())),
            ("Bartonian".to_string(), Some("#00ff00ff".to_string())),
            ("Priabonian".to_string(), Some("#0000ffff".to_string())),
        ]
    );
    assert_eq!(
        strata.units[1].colour_from.as_deref(),
        Some(TABLE),
        "the object that carried the colour is the table"
    );

    let strata = titled(false);
    assert_eq!(strata.units[2].colour, None, "35 has no entry in the map");
    assert!(
        strata
            .notes
            .iter()
            .any(|note| note.contains("no entry for the key 35")),
        "{:?}",
        strata.notes
    );
}

/// A category is its key wherever its map is found: a discrete map on the **property** is read by key too, and the key
/// it has no entry for takes no colour from it (not the colour of the nearest lower entry).
///
/// Break it: in `well_strata::property_map`, write `let by_key = decision.level == Level::CategoryTable;`.
#[test]
fn a_discrete_map_on_the_property_is_read_by_key_too() {
    let mds = [0.0, 5.0, 12.0, 20.0];
    let property = discrete(&[20, 10, 35]);
    let well = frame(&mds);
    let objects = held(vec![
        boxed(well.clone()),
        boxed(table_22(1)),
        boxed(property.clone()),
        boxed(map(MAP_OF_PROPERTY, false)),
    ]);
    let set = vec![(
        "d0000000-0000-4000-8000-0000000000ff".to_string(),
        set_of(&[(PROPERTY, "resqml22.DiscreteProperty", MAP_OF_PROPERTY, None)]),
    )];
    let known = Known {
        graphics: &set,
        units: &[],
    };
    let strata = well_strata::of_frame(&well, &Wanted::Category { property: &property }, &objects, &known)
        .expect("the property names its rows");
    let colours: Vec<Option<String>> = strata
        .units
        .iter()
        .map(|unit| unit.colour.map(|rgba| rgba.to_hex()))
        .collect();
    assert_eq!(
        colours,
        [Some("#ff0000ff".to_string()), Some("#00ff00ff".to_string()), None],
        "35 is above the last entry, 20: it is not green"
    );
}

// -- a package written against the draft of 2.2 --------------------------------------------

const DRAFT_TABLE: &str = "c5543ff9-8ac2-44bd-9c04-a6281c7d8436";
const DRAFT_MAP: &str = "b29851ae-2dd2-4e68-8250-6d6b274fdb45";

/// The three objects as such a package writes them, in the shapes the draft of 2.2 (`v2.2_dev3`) gives: the
/// `ColorInformation` names its map by an element called `DiscreteColorMap` (the released schema calls it `ColorMap`), the
/// prefixes are declared on the entry itself, not on the root, and the table lists its `Column` before its `KeyColumn`.
fn draft_objects() -> Vec<Box<dyn DynObject>> {
    let read = |content_type: &str, xml: &str| {
        energyml_schemas::all()
            .read_xml(content_type, xml.as_bytes())
            .expect("the object reads")
    };
    let table = format!(
        r####"<?xml version="1.0" encoding="UTF-8"?>
<eml:ColumnBasedTable xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" uuid="{DRAFT_TABLE}" schemaVersion="2.3" objectVersion="0">
  <eml:Citation><eml:Title>codes</eml:Title><eml:Originator>test</eml:Originator><eml:Creation>2021-05-18T22:31:17Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>
  <eml:Existence>actual</eml:Existence>
  <eml:Column>
    <eml:ValueCountPerRow>1</eml:ValueCountPerRow>
    <eml:PropertyKind><eml:Uuid>5f78f66a-ed1b-4827-a868-beb989febb31</eml:Uuid><eml:QualifiedType>eml23.PropertyKind</eml:QualifiedType><eml:Title>code</eml:Title></eml:PropertyKind>
    <eml:Values xsi:type="eml:StringXmlArray"><eml:CountPerValue>1</eml:CountPerValue><eml:Values>Q</eml:Values><eml:Values>N1</eml:Values><eml:Values>Socle</eml:Values><eml:Values>Alter_MC</eml:Values><eml:Values>UND</eml:Values></eml:Values>
  </eml:Column>
  <eml:KeyColumn>
    <eml:ValueCountPerRow>1</eml:ValueCountPerRow>
    <eml:PropertyKind><eml:Uuid>2bcbe7d8-cd41-4758-a88f-242781e7f495</eml:Uuid><eml:QualifiedType>eml23.PropertyKind</eml:QualifiedType><eml:Title>classification index</eml:Title></eml:PropertyKind>
    <eml:Values xsi:type="eml:IntegerXmlArray"><eml:CountPerValue>1</eml:CountPerValue><eml:NullValue>-1</eml:NullValue><eml:Values>0 1 2 3 4</eml:Values></eml:Values>
  </eml:KeyColumn>
</eml:ColumnBasedTable>"####
    );
    let entries: String = (0..5)
        .map(|index| {
            format!(
                "<resqml:Entry><resqml:index>{index}</resqml:index><resqml:Hsv><resqml:Alpha>1</resqml:Alpha>\
                 <resqml:Hue>0.{index}</resqml:Hue><resqml:Saturation>1</resqml:Saturation>\
                 <resqml:Value>1</resqml:Value></resqml:Hsv></resqml:Entry>"
            )
        })
        .collect();
    let map = format!(
        r####"<?xml version="1.0" encoding="UTF-8"?>
<resqml:DiscreteColorMap xmlns:resqml="http://www.energistics.org/energyml/data/resqmlv2" xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" uuid="{DRAFT_MAP}" schemaVersion="2.2" objectVersion="0">
  <eml:Citation><eml:Title>colours</eml:Title><eml:Originator>test</eml:Originator><eml:Creation>2021-05-18T22:31:17Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>
  <eml:Existence>actual</eml:Existence>
  {entries}
</resqml:DiscreteColorMap>"####
    );
    vec![
        read("application/x-eml+xml;version=2.3;type=ColumnBasedTable", &table),
        read("application/x-resqml+xml;version=2.2;type=DiscreteColorMap", &map),
    ]
}

fn draft_set() -> GraphicalSet {
    let xml = format!(
        r####"<?xml version="1.0" encoding="UTF-8"?>
<eml:GraphicalInformationSet xmlns:eml="http://www.energistics.org/energyml/data/commonv2" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" uuid="4d4d5555-a3bd-47ef-b559-99c25a9d0558" schemaVersion="2.3" objectVersion="0">
  <eml:Citation><eml:Title>a set</eml:Title><eml:Originator>test</eml:Originator><eml:Creation>2021-05-18T22:34:10Z</eml:Creation><eml:Format>test</eml:Format></eml:Citation>
  <eml:Existence>actual</eml:Existence>
  <eml:GraphicalInformation xsi:type="resqmlv2:ColorInformation" xmlns:commonv2="http://www.energistics.org/energyml/data/commonv2" xmlns:resqmlv2="http://www.energistics.org/energyml/data/resqmlv2"><commonv2:TargetObject><commonv2:Uuid>{DRAFT_TABLE}</commonv2:Uuid><commonv2:ObjectVersion>0</commonv2:ObjectVersion><commonv2:QualifiedType>eml23.ColumnBasedTable</commonv2:QualifiedType><commonv2:Title>codes</commonv2:Title></commonv2:TargetObject><resqmlv2:UseLogarithmicMapping>false</resqmlv2:UseLogarithmicMapping><resqmlv2:UseReverseMapping>false</resqmlv2:UseReverseMapping><resqmlv2:ValueVectorIndex>1</resqmlv2:ValueVectorIndex><resqmlv2:DiscreteColorMap><commonv2:Uuid>{DRAFT_MAP}</commonv2:Uuid><commonv2:ObjectVersion>0</commonv2:ObjectVersion><commonv2:QualifiedType>resqml22.DiscreteColorMap</commonv2:QualifiedType><commonv2:Title>colours</commonv2:Title></resqmlv2:DiscreteColorMap>
  </eml:GraphicalInformation>
</eml:GraphicalInformationSet>"####
    );
    let object = energyml_schemas::all()
        .read_xml(
            "application/x-eml+xml;version=2.3;type=GraphicalInformationSet",
            xml.as_bytes(),
        )
        .expect("the set reads");
    GraphicalSet::read(object.as_reflect())
}

/// The colour map a package written against the draft of 2.2 puts on the table of its categories is **taken**: the
/// entry names its map by `DiscreteColorMap` (not `ColorMap`), its prefixes are its own, `ValueVectorIndex` 1 is the
/// column `Column` - the key column is column 0 - and the map's entries 0 to 4 are the keys of the table. The same
/// answer in every cache that asks the cascade (`Cascade::table_decision` is the one door).
///
/// Break it: in `read_raw_entry`, look for the map under `ColorMap` alone: the entry then names no map, the table has no
/// decision and the categories fall back to a palette.
#[test]
fn a_colour_map_named_by_the_draft_element_is_taken_for_the_table() {
    let set = draft_set();
    let objects = held(draft_objects());
    let decision = Cascade::new(&set)
        .table_decision(DRAFT_TABLE, &objects)
        .expect("column 1 is the `Column`: the key column is column 0")
        .expect("the entry names its map");
    assert_eq!(decision.level, Level::CategoryTable);
    let Paint::Mapped {
        map,
        value_vector_index,
        ..
    } = &decision.paint
    else {
        panic!("a colour map");
    };
    assert_eq!(*value_vector_index, Some(1));
    for key in 0..5 {
        assert!(
            map.sample_key(f64::from(key)).is_some(),
            "key {key} has its entry"
        );
    }
    assert_eq!(map.sample_key(5.0), None, "5 is no key");
}
