//! `collection`: a `DataobjectCollection` and the association set that says
//! what it holds.
//!
//! The form of each assertion comes from `Collection.xsd` (eml 2.3):
//! `SingleCollectionAssociation` has `Collection` 1..1 and `Dataobject`
//! 1..∞; *“If two or more of the same data objects are used in one
//! SingleCollectionAssociation, only one data object should be taken into
//! account”*; `HomogeneousDatatype`, *“If true all data objects in the
//! collection are of the same Energistics data type”*. The uuids and titles
//! are the test's own.

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};
use serde_json::{Value, json};

fn when() -> When {
    When { creation: "2026-09-24T00:00:00Z".into(), originator: "test".into() }
}

fn object(uuid: &str, qualified_type: &str, title: &str) -> Source {
    Source::Object(Known {
        reference: Some(Ref { uuid: uuid.into(), qualified_type: qualified_type.into(), title: title.into(), object_version: None }),
        ..Default::default()
    })
}

const A: &str = "2a9f3c74-5e6f-4071-8cbd-2e3f4a5b6c7d";
const B: &str = "3b0a4d85-6f70-4182-9dce-3f4a5b6c7d8e";

fn two_wells() -> Pieces {
    Pieces::empty()
        .with("members", object(A, "resqml22.WellboreInterpretation", "GTH-1"))
        .with("members", object(B, "resqml22.WellboreInterpretation", "GTH-2"))
}

fn planned(pieces: &Pieces, choices: Choices) -> Result<Plan, String> {
    Registry::new()
        .plan("collection", pieces, &choices, &Identity::derived(), &when())
        .map_err(|why| why.to_string())
}

fn of_type<'a>(plan: &'a Plan, qualified_type: &str) -> &'a Value {
    &plan.objects.iter().find(|one| one.qualified_type == qualified_type).expect(qualified_type).json
}

#[test]
fn the_association_names_the_collection_and_every_member() {
    let plan = planned(&two_wells(), Choices::new().set("title", "wells").set("kind", "folder")).expect("planned");
    let collection = plan.objects.iter().find(|one| one.qualified_type == "eml23.DataobjectCollection").unwrap();
    let set = of_type(&plan, "eml23.CollectionsToDataobjectsAssociationSet");
    let association = &set["SingleCollectionAssociation"][0];
    assert_eq!(association["Collection"]["Uuid"], json!(collection.uuid), "{set}");
    let members: Vec<&str> =
        association["Dataobject"].as_array().expect("Dataobject").iter().map(|m| m["Uuid"].as_str().unwrap()).collect();
    assert_eq!(members, vec![A, B], "{set}");
    assert_eq!(collection.json["Kind"], json!("folder"), "{}", collection.json);
}

#[test]
fn a_member_given_twice_is_written_once_and_counted() {
    let pieces = two_wells().with("members", object(A, "resqml22.WellboreInterpretation", "GTH-1"));
    let plan = planned(&pieces, Choices::new().set("title", "wells")).expect("planned");
    let set = of_type(&plan, "eml23.CollectionsToDataobjectsAssociationSet");
    assert_eq!(set["SingleCollectionAssociation"][0]["Dataobject"].as_array().unwrap().len(), 2, "{set}");
    let twice = plan.says.iter().find(|note| note.key == "twice").expect("a twice note");
    assert!(twice.detail.contains("`GTH-1` ×2"), "the repeated member is named: {}", twice.detail);
}

#[test]
fn homogeneous_is_what_the_members_are_and_a_requirement_is_checked() {
    let same = planned(&two_wells(), Choices::new().set("title", "wells").set("homogeneous", true)).expect("planned");
    assert_eq!(of_type(&same, "eml23.DataobjectCollection")["HomogeneousDatatype"], json!(true));

    let mixed = two_wells().with("members", object("4c1b5e96-7081-4293-aedf-4a5b6c7d8e9f", "resqml22.Model", "M"));
    let plan = planned(&mixed, Choices::new().set("title", "all")).expect("planned");
    assert_eq!(of_type(&plan, "eml23.DataobjectCollection")["HomogeneousDatatype"], json!(false));
    let why = planned(&mixed, Choices::new().set("title", "all").set("homogeneous", true)).unwrap_err();
    assert!(why.contains("resqml22.Model") && why.contains("resqml22.WellboreInterpretation"), "{why}");
}

#[test]
fn a_collection_without_members_is_refused() {
    assert!(planned(&Pieces::empty(), Choices::new().set("title", "empty")).is_err());
}

#[test]
fn the_same_collection_keeps_its_uuid_and_other_members_make_another_set() {
    let one = planned(&two_wells(), Choices::new().set("title", "wells")).unwrap();
    let fewer = planned(
        &Pieces::empty().with("members", object(A, "resqml22.WellboreInterpretation", "GTH-1")),
        Choices::new().set("title", "wells"),
    )
    .unwrap();
    let uuid = |plan: &Plan, t: &str| plan.objects.iter().find(|o| o.qualified_type == t).unwrap().uuid.clone();
    assert_eq!(uuid(&one, "eml23.DataobjectCollection"), uuid(&fewer, "eml23.DataobjectCollection"));
    assert_ne!(
        uuid(&one, "eml23.CollectionsToDataobjectsAssociationSet"),
        uuid(&fewer, "eml23.CollectionsToDataobjectsAssociationSet")
    );
}
