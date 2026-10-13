//! `edit-object`: an object of the package with fields set as told —
//! rewritten under its own uuid, or copied under a new one.
//!
//! Pure plans over hand-made objects: what the operation is handed is
//! `Known::json`, which the package's reader fills, and here the test does. The
//! objects, uuids and titles are the test's own; the shapes follow the
//! workspace's JSON encoding (`energyml_core::json`) — `ContactElement` and
//! `Qualifier` as RESQML 2.2 Interpretations.xsd writes a contact.

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use serde_json::{Value, json};

const FAULT: &str = "6d1e2f3a-4b5c-4d6e-8f70-81a2b3c4d5e6";
const SURFACE: &str = "7e2f3a4b-5c6d-4e7f-9081-92b3c4d5e6f7";

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A fault interpretation, normal and listric — what `fault-from-sticks` writes.
fn fault() -> Value {
    json!({
        "$type": "resqml22.FaultInterpretation",
        "uuid": FAULT,
        "schemaVersion": "2.2",
        "Citation": { "$type": "eml23.Citation", "Title": "F9 — interpretation", "Originator": "someone", "Creation": "2026-01-01T00:00:00Z" },
        "IsListric": true,
        "ThrowInterpretation": [ { "$type": "resqml22.FaultThrow", "Throw": ["normal"] } ],
        "ContactInterpretation": [ {
            "$type": "resqml22.BinaryContactInterpretationPart",
            "Subject": { "$type": "resqml22.ContactElement", "Uuid": FAULT, "QualifiedType": "resqml22.FaultInterpretation", "Title": "F9 — interpretation", "Qualifier": "footwall" },
            "Verb": "stops"
        } ],
    })
}

fn bag(json: Option<Value>) -> Pieces {
    Pieces::empty().with(
        "object",
        Source::Object(Known {
            reference: Some(Ref {
                uuid: FAULT.into(),
                qualified_type: "resqml22.FaultInterpretation".into(),
                title: "F9 — interpretation".into(),
                object_version: None,
            }),
            json,
            ..Default::default()
        }),
    )
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("edit-object", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn one(plan: &Plan) -> &Value {
    assert_eq!(plan.objects.len(), 1, "one object written");
    &plan.objects[0].json
}

#[test]
fn in_place_the_fields_change_and_nothing_else() {
    let plan = planned(
        &bag(Some(fault())),
        Choices::new().set("fields", json!(["ThrowInterpretation[0].Throw = [\"reverse\"]"])),
    )
    .expect("planned");
    let written = one(&plan);
    assert_eq!(plan.objects[0].uuid, FAULT, "in place, the uuid is the object's");
    assert_eq!(written["ThrowInterpretation"][0]["Throw"], json!(["reverse"]));
    // Everything else as it was.
    assert_eq!(written["IsListric"], json!(true));
    assert_eq!(written["ContactInterpretation"], fault()["ContactInterpretation"]);
    assert_eq!(written["Citation"]["Title"], json!("F9 — interpretation"));
    assert_eq!(
        written["Citation"]["Creation"],
        json!("2026-01-01T00:00:00Z"),
        "the object was made then"
    );
    // The citation says it was edited, and by whom.
    assert_eq!(written["Citation"]["LastUpdate"], json!("2026-09-25T00:00:00Z"));
    assert_eq!(written["Citation"]["Editor"], json!("test"));
    // And the plan says what changed, old and new.
    let said = plan
        .says
        .iter()
        .find(|note| note.key == "set")
        .expect("a set note");
    assert!(
        said.detail.contains("[\"normal\"]") && said.detail.contains("[\"reverse\"]"),
        "{}",
        said.detail
    );
}

#[test]
fn a_copy_has_its_own_uuid_title_and_creation_and_the_same_one_every_run() {
    let choices = || {
        Choices::new()
            .set("copy", "F9 bis")
            .set("fields", json!(["IsListric = null"]))
    };
    let first = planned(&bag(Some(fault())), choices()).expect("planned");
    let again = planned(&bag(Some(fault())), choices()).expect("planned");
    let copy = &first.objects[0];
    assert_ne!(copy.uuid, FAULT);
    assert_eq!(
        copy.uuid, again.objects[0].uuid,
        "the same copy is the same object from one run to the next"
    );
    assert_eq!(copy.json["uuid"], json!(copy.uuid));
    assert_eq!(copy.title, "F9 bis");
    assert_eq!(copy.json["Citation"]["Title"], json!("F9 bis"));
    assert_eq!(copy.json["Citation"]["Creation"], json!("2026-09-25T00:00:00Z"));
    assert_eq!(copy.json["Citation"]["Originator"], json!("test"));
    assert!(copy.json["Citation"].get("LastUpdate").is_none());
    // `null` removed the field.
    assert!(copy.json.get("IsListric").is_none(), "{}", copy.json);
}

#[test]
fn a_reference_replaces_what_is_named_and_keeps_what_it_is() {
    let surface = json!({ "$type": "eml23.DataObjectReference", "Uuid": SURFACE, "QualifiedType": "resqml22.TriangulatedSetRepresentation", "Title": "F9" });
    let plan = planned(
        &bag(Some(fault())),
        Choices::new()
            .set("reference", surface)
            .set("referenceAt", "ContactInterpretation[0].Subject"),
    )
    .expect("planned");
    let subject = &one(&plan)["ContactInterpretation"][0]["Subject"];
    assert_eq!(
        subject,
        &json!({ "$type": "resqml22.ContactElement", "Uuid": SURFACE, "QualifiedType": "resqml22.TriangulatedSetRepresentation", "Title": "F9", "Qualifier": "footwall" }),
        "still a ContactElement, with its qualifier"
    );
}

#[test]
fn what_cannot_be_done_is_refused_by_name() {
    let refused = |pieces: &Pieces, choices: Choices| planned(pieces, choices).expect_err("refused");
    let set = |fields: &[&str]| Choices::new().set("fields", json!(fields));

    // An object handed without its content: not a package's reader.
    assert!(refused(&bag(None), set(&["IsListric = false"])).contains("without its content"));
    // A title is not rewritten in place: every reference holds a copy.
    assert!(refused(&bag(Some(fault())), set(&["Citation.Title = \"F10\""])).contains("stale"));
    // A step that leads nowhere is never created on the way.
    assert!(refused(&bag(Some(fault())), set(&["Nowhere.Throw = 1"])).contains("has no `Nowhere`"));
    // The identity and the type are not fields.
    assert!(refused(&bag(Some(fault())), set(&["uuid = \"x\""])).contains("identity"));
    // Text that is not JSON is not taken for a string.
    assert!(refused(&bag(Some(fault())), set(&["IsListric = maybe"])).contains("not JSON"));
    // Twice the same path.
    assert!(
        refused(
            &bag(Some(fault())),
            set(&["IsListric = true", "IsListric = false"])
        )
        .contains("set twice")
    );
    // Nothing to set.
    assert!(refused(&bag(Some(fault())), Choices::new()).contains("nothing to set"));
    // A reference without its place, a place without its reference.
    let surface = json!({ "$type": "eml23.DataObjectReference", "Uuid": SURFACE, "QualifiedType": "resqml22.TriangulatedSetRepresentation", "Title": "F9" });
    assert!(
        refused(&bag(Some(fault())), Choices::new().set("reference", surface)).contains("nothing says where")
    );
    assert!(
        refused(
            &bag(Some(fault())),
            Choices::new().set("referenceAt", "ContactInterpretation[0].Subject")
        )
        .contains("none is given")
    );
}
