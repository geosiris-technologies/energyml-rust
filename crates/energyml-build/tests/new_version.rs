//! `new-version`: an object copied under another `objectVersion` - the test copy
//! needed to test identity (ADR 0003), a copy of an
//! object with a different version.
//!
//! The plan is pure, over a hand-made object (`Known::json`, filled by the
//! package's reader in real use); `apply` is run against a package opened in
//! memory, which is where the rule shows: accept the short uri when
//! there is no conflict (or when an object has no version), and the form with
//! the version when there is one.

use std::sync::Arc;

use energyml_build::apply::apply;
use energyml_build::operation::{Choices, Identity, NEW_VERSION_ROLE, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use serde_json::{Value, json};

const SURFACE: &str = "7d000000-0000-4000-8000-000000000001";
const TYPE: &str = "resqml22.BoundaryFeature";

fn when() -> When {
    When {
        creation: "2026-10-05T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// A surface's feature: no `objectVersion` - what every writer of this crate
/// produces.
fn surface() -> Value {
    json!({
        "$type": TYPE,
        "uuid": SURFACE,
        "schemaVersion": "2.2",
        "Citation": { "$type": "eml23.Citation", "Title": "H3 - top of the Ypresian", "Originator": "someone", "Creation": "2026-01-01T00:00:00Z" },
        "IsWellKnown": false,
    })
}

fn bag(json: Option<Value>) -> Pieces {
    Pieces::empty().with(
        "object",
        Source::Object(Known {
            reference: Some(Ref {
                uuid: SURFACE.into(),
                qualified_type: TYPE.into(),
                title: "H3 - top of the Ypresian".into(),
                object_version: None,
            }),
            json,
            ..Default::default()
        }),
    )
}

fn planned(pieces: &Pieces, version: &str) -> Result<Plan, String> {
    Registry::new()
        .plan(
            "new-version",
            pieces,
            &Choices::new().set("version", version),
            &Identity::derived(),
            &when(),
        )
        .map_err(|why| why.to_string())
}

/// What would have to break: the plan changing the uuid (it would be a copy, not
/// a version), the title (every reference holds a copy of it, and each would go
/// stale), or writing the object under the ordinary role (an ordinary put
/// replaces).
#[test]
fn the_plan_keeps_the_identity_and_changes_the_version_only() {
    let plan = planned(&bag(Some(surface())), "2").expect("planned");
    assert_eq!(plan.objects.len(), 1);
    let copy = &plan.objects[0];
    assert_eq!(copy.uuid, SURFACE, "the same uuid");
    assert_eq!(copy.qualified_type, TYPE, "the same type");
    assert_eq!(
        copy.role, NEW_VERSION_ROLE,
        "written through the gate that adds a version"
    );
    assert_eq!(copy.title, "H3 - top of the Ypresian", "the same title");
    assert_eq!(copy.json["objectVersion"], json!("2"));
    assert_eq!(copy.json["Citation"]["Title"], json!("H3 - top of the Ypresian"));
    assert_eq!(
        copy.json["Citation"]["Creation"],
        json!("2026-01-01T00:00:00Z"),
        "made then"
    );
    assert_eq!(copy.json["Citation"]["LastUpdate"], json!("2026-10-05T00:00:00Z"));
    assert_eq!(
        copy.json["IsWellKnown"],
        json!(false),
        "the content is the object's"
    );
    let said = plan
        .says
        .iter()
        .find(|note| note.key == "new-version")
        .expect("a note");
    assert!(said.detail.contains("keeps the short uri"), "{}", said.detail);
}

/// RESQML 2.0.1 keeps the version in the citation.
#[test]
fn a_resqml_2_0_1_object_gets_its_version_in_the_citation() {
    let mut old = surface();
    old["$type"] = json!("resqml20.obj_BoundaryFeature");
    old["Citation"]["$type"] = json!("eml20.Citation");
    let pieces = Pieces::empty().with(
        "object",
        Source::Object(Known {
            reference: Some(Ref {
                uuid: SURFACE.into(),
                qualified_type: "resqml20.obj_BoundaryFeature".into(),
                title: "H3".into(),
                object_version: None,
            }),
            json: Some(old),
            ..Default::default()
        }),
    );
    let plan = planned(&pieces, "2").expect("planned");
    assert_eq!(plan.objects[0].json["Citation"]["VersionString"], json!("2"));
    assert!(plan.objects[0].json.get("objectVersion").is_none());
}

#[test]
fn what_cannot_be_done_is_refused_by_name() {
    let refused = |pieces: &Pieces, version: &str| planned(pieces, version).expect_err("refused");
    assert!(refused(&bag(None), "2").contains("without its content"));
    assert!(refused(&bag(Some(surface())), "  ").contains("empty objectVersion"));
    let mut versioned = surface();
    versioned["objectVersion"] = json!("2");
    let why = refused(&bag(Some(versioned)), "2");
    assert!(why.contains("already carries the objectVersion `2`"), "{why}");
}

/// The whole path: the original keeps its short uri, the copy takes the form
/// with its version, each reads as itself - and played again, the copy is refused
/// by name rather than written over the first.
#[test]
fn applied_the_original_keeps_the_short_uri_and_the_copy_takes_the_versioned_one() {
    let registry = Arc::new(energyml_schemas::all());
    let mut store = energyml_build::epc::open(None, "versions.epc", registry.clone()).expect("a new package");
    // The original, put as any writer of this crate puts it.
    let original = energyml_core::json::read_string(&surface().to_string(), &registry)
        .expect("reads")
        .root;
    let short = store.put(original).expect("put");
    assert_eq!(short, format!("eml:///{TYPE}({SURFACE})"));

    let plan = planned(&bag(Some(surface())), "2").expect("planned");
    let made = apply(&plan, &mut store, &when());
    assert!(made.refused.is_empty(), "{:?}", made.refused);
    assert!(made.invalid.is_empty(), "{:?}", made.invalid);
    let versioned = format!("eml:///{TYPE}(uuid={SURFACE},version='2')");
    assert!(made.written.contains(&versioned), "{:?}", made.written);
    assert!(
        !made.replaced.contains(&versioned),
        "nothing was replaced: {:?}",
        made.replaced
    );

    // The rule, on the package the operation wrote.
    let held: Vec<String> = store
        .resources()
        .into_iter()
        .filter(|one| one.qualified_type == TYPE)
        .map(|one| one.uri)
        .collect();
    assert_eq!(held.len(), 2, "{held:?}");
    assert!(
        held.contains(&short),
        "the original keeps the short uri: {held:?}"
    );
    assert!(held.contains(&versioned), "{held:?}");
    assert_eq!(store.resolve(&short).unwrap().uri, short);
    assert_eq!(store.resolve(&versioned).unwrap().uri, versioned);
    let copy = store.get_object(&versioned).expect("reads");
    assert_eq!(
        energyml_core::identity::object_version_of_reflect(copy.as_reflect()),
        Some("2")
    );
    let first = store.get_object(&short).expect("reads");
    assert_eq!(
        energyml_core::identity::object_version_of_reflect(first.as_reflect()),
        None
    );

    // Again: the exact identity is refused, by name, and nothing is written.
    let again = apply(&plan, &mut store, &when());
    assert!(
        again.refused.iter().any(|why| why.contains("already held")),
        "{:?}",
        again.refused
    );
    assert_eq!(
        store
            .resources()
            .into_iter()
            .filter(|one| one.qualified_type == TYPE)
            .count(),
        2,
        "the package still holds the two"
    );
}
