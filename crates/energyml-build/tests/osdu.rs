//! The OSDU template: its three forms, its key, and what it refuses.
//!
//! The shape of the assertions comes from the schemas
//! `master-data/ColumnBasedTableTemplate.1.0.1`,
//! `abstract/AbstractReferencePropertyType.1.2.1` and
//! `dataset/File.TabularData.1.0.0`; the values come from the files.

use energyml_build::osdu::{self, ValueType};
use serde_json::json;

/// A minimal entry.
fn entry(name: &str, uuid: &str) -> serde_json::Value {
    json!({
        "ColumnName": name,
        "ValueType": "number",
        "UnitOfMeasureID": "namespace:reference-data--UnitOfMeasure:m:",
        "PropertyType": { "PropertyTypeID": format!("namespace:reference-data--PropertyType:{uuid}:"), "Name": "measured depth" },
    })
}

#[test]
fn a_bare_list_is_a_template() {
    // *“or simply a list of the columns of this template”* - and the schema
    // confirms it: `Columns[]` is a list of `AbstractReferencePropertyType`,
    // so the list alone loses nothing.
    let read =
        osdu::from_json(&json!([entry("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5")])).expect("reads");
    assert_eq!(read.columns.len(), 1);
    assert_eq!(read.columns[0].name, "PROF");
    assert_eq!(
        read.columns[0].kind.as_deref(),
        Some("c48c65d4-9680-4d10-903c-a1b2e30f66b5")
    );
    assert_eq!(read.columns[0].uom.as_deref(), Some("m"));
    assert_eq!(read.columns[0].value_type, ValueType::Number);
}

#[test]
fn a_whole_template_record_reads_the_same() {
    let record = json!({
        "kind": "osdu:wks:master-data--ColumnBasedTableTemplate:1.0.1",
        "data": { "Name": "forages", "Columns": [entry("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5")] },
    });
    let read = osdu::from_json(&record).expect("reads");
    assert_eq!(read.columns.len(), 1);
    assert_eq!(read.columns[0].name, "PROF");
}

#[test]
fn key_columns_are_columns_too() {
    // `KeyColumns[]` is of the same kind, and describes the same cells: keeping
    // them apart would mean that a key is never matched to its header.
    let record = json!({
        "data": {
            "KeyColumns": [entry("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1")],
            "Columns": [entry("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5")],
        }
    });
    let read = osdu::from_json(&record).expect("reads");
    assert_eq!(read.columns.len(), 2);
    assert_eq!(read.keys, ["SONDAGE"]);
}

#[test]
fn a_tabular_data_record_says_how_the_file_reads() {
    // Where the CBTT says what the columns mean, this one says how the
    // file reads - and carries `AbsentValue`, the sentinel that would otherwise be read
    // as a measurement.
    let record = json!({
        "data": {
            "ColumnSeparator": ";",
            "ColumnNamesInFirstRow": true,
            "NumberOfRowsToSkip": 2,
            "CommentIndicator": "%",
            "Columns": [entry("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5")],
            "ColumnSpecifications": [{ "ColumnName": "PROF", "AbsentValue": "-999.25" }],
        }
    });
    let read = osdu::from_json(&record).expect("reads");
    let dialect = read.dialect.expect("a TabularData states its dialect");
    assert_eq!(dialect.separator, ';');
    assert_eq!(dialect.skip, 2);
    assert_eq!(dialect.comment, Some('%'));
    assert_eq!(read.columns[0].absent.as_deref(), Some("-999.25"));
}

#[test]
fn an_entry_without_a_column_name_is_refused() {
    // The schema says it is optional, its own description says it is mandatory:
    // *“the data.Columns[].ColumnName is mandatory although it is defined as
    // optional in the schema”*. Un validateur JSON Schema laisserait passer.
    let why = osdu::from_json(&json!([{ "ValueType": "number" }])).expect_err("refuses");
    assert!(why.why.contains("ColumnName"), "{}", why.why);
}

#[test]
fn the_uuid_is_found_whatever_the_namespace() {
    // The namespace is free and the version suffix may be missing: the uuid is
    // looked for **by its shape**, not by its position.
    for id in [
        "namespace:reference-data--PropertyType:c48c65d4-9680-4d10-903c-a1b2e30f66b5:",
        "osdu:reference-data--PropertyType:c48c65d4-9680-4d10-903c-a1b2e30f66b5",
        "a-b.c:reference-data--PropertyType:C48C65D4-9680-4D10-903C-A1B2E30F66B5:42",
    ] {
        let read = osdu::from_json(&json!([{
            "ColumnName": "PROF",
            "PropertyType": { "PropertyTypeID": id },
        }]))
        .expect("reads");
        assert_eq!(
            read.columns[0].kind.as_deref(),
            Some("c48c65d4-9680-4d10-903c-a1b2e30f66b5"),
            "{id}"
        );
    }
}

#[test]
fn a_column_without_a_kind_is_not_an_error() {
    // A column that the template does not describe is ignored and counted, never
    // a refusal: a business file always carries columns that are no
    // concern of ours.
    let read = osdu::from_json(&json!([{ "ColumnName": "Commentaire" }])).expect("reads");
    assert_eq!(read.columns[0].kind, None);
}

#[test]
fn two_columns_for_one_kind_are_not_guessed_between() {
    // Choosing the abscissa of a borehole at random between two columns would be the
    // fault that this crate refuses everywhere.
    let read = osdu::from_json(&json!([
        entry("X", "620cec71-5ce3-4ea5-9120-833756b427d7"),
        entry("XX", "620cec71-5ce3-4ea5-9120-833756b427d7"),
    ]))
    .expect("reads");
    let why = read
        .for_kind("620cec71-5ce3-4ea5-9120-833756b427d7")
        .expect_err("refuses to pick");
    assert!(why.contains("`X`") && why.contains("`XX`"), "{why}");
}

#[test]
fn a_unit_on_a_text_column_is_said_and_not_refused() {
    // The template of the precedent does it thirteen times. No effect on the reading,
    // but it is a template that another tool will read.
    let read = osdu::from_json(&json!([{
        "ColumnName": "Z_MNT",
        "ValueType": "string",
        "UnitOfMeasureID": "namespace:reference-data--UnitOfMeasure:m:",
    }]))
    .expect("reads");
    assert!(read.notes.iter().any(|n| n.contains("Z_MNT")), "{:?}", read.notes);
}

#[test]
fn what_this_module_writes_it_reads_back() {
    // **The round trip, and it is necessary rather than prudent.** A front end reads
    // a template, shows it, modifies it and **sends it back**; if the return
    // were not read, setting its columns on screen then planning
    // would answer *“this is neither a list of column definitions nor…”* on
    // the template that the front end has itself just written. That was the defect of
    // 22/09.
    //
    // To make it turn red: remove the compact branch of `column`.
    let osdu = osdu::from_json(&json!({
        "data": {
            "KeyColumns": [entry("SONDAGE", "348c9a4d-e3e4-4beb-8456-f07a46660de1")],
            "Columns": [entry("PROF", "c48c65d4-9680-4d10-903c-a1b2e30f66b5")],
        }
    }))
    .expect("reads");

    // What the wasm face writes of it, field for field.
    let written = json!({
        "columns": osdu
            .columns
            .iter()
            .map(|c| json!({
                "name": c.name,
                "kind": c.kind,
                "label": c.label,
                "valueType": c.value_type.word(),
                "count": c.count,
                "uom": c.uom,
                "quantity": c.quantity,
                "facets": c.facets,
                "absent": c.absent,
            }))
            .collect::<Vec<_>>(),
        "keys": osdu.keys,
        "notes": osdu.notes,
    });

    let again = osdu::from_json(&written).expect("the reduced form reads back");
    assert_eq!(again.columns.len(), osdu.columns.len());
    assert_eq!(again.keys, osdu.keys, "the keys survive the round trip");
    for (before, after) in osdu.columns.iter().zip(again.columns.iter()) {
        assert_eq!(after.name, before.name);
        assert_eq!(
            after.kind, before.kind,
            "the uuid is the key: losing it loses everything"
        );
        assert_eq!(after.uom, before.uom);
        assert_eq!(after.value_type, before.value_type);
    }
}

#[test]
fn a_date_declared_as_text_is_not_a_reason_to_shout() {
    // A feedback of 22/09: *“the dates declared as strings cause a
    // problem whereas they are just defined as in the resqml objects
    // as text. It is not necessarily a number.”*
    //
    // The remark of the schema remains true - `UnitOfMeasureID` is reserved there for
    // `number` - but it describes the **template**, not the file, and it blocks
    // nothing: the template reads, the column keeps its unit. This check
    // pins both halves, because hardening one into a refusal would break a
    // template that works.
    let read = osdu::from_json(&json!([{
        "ColumnName": "DateFin",
        "ValueType": "string",
        "UnitOfMeasureID": "namespace:reference-data--UnitOfMeasure:time:",
        "PropertyType": { "PropertyTypeID": "namespace:reference-data--PropertyType:0d4ae5d0-b3d5-49e4-903c-254b9b3dffda:" },
    }]))
    .expect("a template that declares a date as text reads");
    assert_eq!(read.columns[0].uom.as_deref(), Some("time"), "the unit is kept");
    assert_eq!(read.columns[0].value_type, ValueType::Text);
    assert!(
        read.notes.iter().any(|n| n.contains("DateFin")),
        "and the remark is made, without refusing"
    );
    // **And it is carried by its column**, so that a front end can mark
    // the row without searching for its name in the sentence - which would be rereading our
    // own messages.
    assert_eq!(read.columns[0].notes.len(), 1, "the remark belongs to `DateFin`");
}

#[test]
fn a_columns_remark_survives_the_round_trip() {
    // Otherwise the row would be marked at the first load and would stop
    // being so as soon as another column is set - the mark would blink without
    // anything having changed in the template.
    let read = osdu::from_json(&json!([{
        "ColumnName": "Z_MNT",
        "ValueType": "string",
        "UnitOfMeasureID": "namespace:reference-data--UnitOfMeasure:m:",
    }]))
    .expect("reads");
    let again = osdu::from_json(&json!({
        "columns": [{
            "name": read.columns[0].name,
            "kind": read.columns[0].kind,
            "valueType": read.columns[0].value_type.word(),
            "uom": read.columns[0].uom,
            "notes": read.columns[0].notes,
        }],
        "keys": [],
        "notes": read.notes,
    }))
    .expect("reads back");
    assert_eq!(again.columns[0].notes, read.columns[0].notes);
}
