//! The catalogue's format travels as JSON (ADR-0015): what goes
//! out must come back unchanged, in the camelCase the front end reads.

use energyml_section::model::{
    Asked, CrossWidths, Crosses, Inputs, SectionKey, System, object_key,
};

fn inputs() -> Inputs {
    Inputs {
        trace: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]],
        system: Some(System { epsg: 2154, unit: "m".to_string() }),
        left: 20.0,
        right: 35.0,
        crosses: Crosses::Every {
            spacing: 25.0,
            common: CrossWidths { before: 40.0, after: 60.0, left: 5.0, right: 5.0 },
            own: Vec::new(),
        },
        objects: vec![Asked {
            store: "s1".to_string(),
            revision: 3,
            wells: vec!["eml:///resqml22.WellboreFrameRepresentation(a)".to_string()],
            surfaces: Vec::new(),
        }],
        time_factor: None,
    }
}

#[test]
fn the_inputs_come_back_from_their_json() {
    let written = serde_json::to_string(&inputs()).expect("serialises");
    let read: Inputs = serde_json::from_str(&written).expect("reads back");
    assert_eq!(read, inputs());
}

#[test]
fn the_json_is_camel_case_and_tagged_the_way_the_page_reads_it() {
    let value = serde_json::to_value(inputs()).expect("serialises");
    assert_eq!(value["timeFactor"], serde_json::Value::Null);
    assert_eq!(value["crosses"]["kind"], "every");
    assert_eq!(value["objects"][0]["store"], "s1");
    let key = serde_json::to_value(SectionKey::Cross { index: 2 }).expect("serialises");
    assert_eq!(key, serde_json::json!({ "kind": "cross", "index": 2 }));
}

#[test]
fn an_object_key_joins_store_and_uri_with_a_nul() {
    // The key is the store, a NUL, then the uri.
    assert_eq!(object_key("s1", "eml:///x"), "s1\u{0}eml:///x");
}
