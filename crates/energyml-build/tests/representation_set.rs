//! `representation-set` — a `RepresentationSetRepresentation` of given
//! representations.
//!
//! The form of the assertions comes from what the TRG and the usage guide
//! state: a set holds *“1..*”* `AbstractRepresentation` (RESQML TRG 2.2 §8.8
//! p. 245); its index is *“implicit. It follows the order on which the
//! representations are given”* (Technical Usage Guide 2.2, draft, §4.7 p. 42);
//! `IsHomogeneous` says *“that all of the selected representations are of a
//! single kind”* (TRG §8.8). The objects — titles, uuids — are the tests' own.
//! The written set is read back and checked by `energyml check` in
//! `tests/blocked_wellbore.rs`, beside real members.
//!
//! # What would have to break to make it turn red
//!
//! Writing `IsHomogeneous` from the answer rather than from the members, or
//! hard-coding it true; sorting or de-duplicating the members; accepting an object that
//! is not a representation; taking the list or the title out of the seed.

use serde_json::json;

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};

fn when() -> When {
    When { creation: "2026-09-25T00:00:00Z".into(), originator: "test".into() }
}

/// A representation of the package, by its type and title.
fn member(rank: usize, qualified_type: &str, title: &str) -> Source {
    Source::Object(Known {
        reference: Some(Ref::new(format!("5e7a0000-0000-4000-8000-{rank:012}"), qualified_type, title)),
        ..Default::default()
    })
}

/// The members, in the order given, named as the front end names them.
fn bag(members: &[Source]) -> Pieces {
    let mut pieces = Pieces::empty();
    for (rank, one) in members.iter().enumerate() {
        let name = if rank == 0 { "representations".to_string() } else { format!("representations {}", rank + 1) };
        pieces = pieces.with(name, one.clone());
    }
    pieces
}

fn chalk_grids() -> Vec<Source> {
    vec![
        member(1, "resqml22.IjkGridRepresentation", "Ypresian–Chalk grid"),
        member(2, "resqml22.IjkGridRepresentation", "GTH-1 refined window"),
        member(3, "resqml22.TruncatedIjkGridRepresentation", "Calcaire grossier truncated"),
        member(4, "resqml22.UnstructuredGridRepresentation", "GTH-2 drain"),
    ]
}

fn planned(pieces: &Pieces, choices: &Choices) -> Result<Plan, String> {
    Registry::new().plan("representation-set", pieces, choices, &Identity::derived(), &when()).map_err(|why| why.message())
}

fn the_set(plan: &Plan) -> &serde_json::Value {
    &plan.of_role("representation").next().expect("the set").json
}

/// **`IsHomogeneous` is what the members are.** The reference model groups four grids of
/// three types: false. Two IJK grids: true. The members are written in the
/// order given, every one of them.
#[test]
fn is_homogeneous_is_what_the_members_are() {
    let choices = Choices::new().set("title", "Chalk grids");
    let plan = planned(&bag(&chalk_grids()), &choices).unwrap_or_else(|why| panic!("{why}"));
    let set = the_set(&plan);
    assert_eq!(set["$type"], "resqml22.RepresentationSetRepresentation");
    assert_eq!(set["IsHomogeneous"], json!(false));
    let written: Vec<&str> = set["Representation"].as_array().unwrap().iter().map(|dor| dor["Title"].as_str().unwrap()).collect();
    assert_eq!(written, vec!["Ypresian–Chalk grid", "GTH-1 refined window", "Calcaire grossier truncated", "GTH-2 drain"]);

    let plan = planned(&bag(&chalk_grids()[..2]), &choices).unwrap_or_else(|why| panic!("{why}"));
    assert_eq!(the_set(&plan)["IsHomogeneous"], json!(true));
    // 2.0.1's `obj_` is the same class: still one kind.
    let mixed = vec![member(1, "resqml22.IjkGridRepresentation", "a"), member(2, "resqml20.obj_IjkGridRepresentation", "b")];
    assert_eq!(the_set(&planned(&bag(&mixed), &choices).unwrap())["IsHomogeneous"], json!(true));
}

/// **A stated answer is checked, either way**, and refused with what the
/// members are.
#[test]
fn a_stated_homogeneity_that_disagrees_is_refused_either_way() {
    let why = planned(&bag(&chalk_grids()), &Choices::new().set("title", "Chalk grids").set("homogeneous", true)).unwrap_err();
    assert!(why.contains("`homogeneous`") && why.contains("resqml22.UnstructuredGridRepresentation"), "{why}");
    let why = planned(&bag(&chalk_grids()[..2]), &Choices::new().set("title", "Two grids").set("homogeneous", false)).unwrap_err();
    assert!(why.contains("`homogeneous`") && why.contains("resqml22.IjkGridRepresentation"), "{why}");
    // Agreeing is no refusal.
    assert!(planned(&bag(&chalk_grids()[..2]), &Choices::new().set("title", "Two grids").set("homogeneous", true)).is_ok());
}

/// **A place is an index**: a member given twice would hold two, refused
/// naming both places; and nothing but a representation is a member.
#[test]
fn a_member_twice_or_no_representation_is_refused_by_name() {
    let mut twice = chalk_grids();
    twice.push(member(2, "resqml22.IjkGridRepresentation", "GTH-1 refined window"));
    let why = planned(&bag(&twice), &Choices::new().set("title", "Chalk grids")).unwrap_err();
    assert!(why.contains("GTH-1 refined window") && why.contains("places 2 and 5"), "{why}");

    let mut odd = chalk_grids();
    odd.insert(1, member(9, "resqml22.HorizonInterpretation", "H6"));
    let why = planned(&bag(&odd), &Choices::new().set("title", "Chalk grids")).unwrap_err();
    assert!(why.contains("`H6`") && why.contains("resqml22.HorizonInterpretation"), "{why}");
}

/// **The same bag is the same set**, and another order another set: the order
/// is the index a framework's contacts name.
#[test]
fn the_same_bag_is_the_same_set_and_another_order_another() {
    let choices = Choices::new().set("title", "Chalk grids");
    let uuid = |members: &[Source], choices: &Choices| the_set(&planned(&bag(members), choices).unwrap())["uuid"].clone();
    let first = uuid(&chalk_grids(), &choices);
    assert_eq!(first, uuid(&chalk_grids(), &choices));
    let mut reversed = chalk_grids();
    reversed.reverse();
    assert_ne!(first, uuid(&reversed, &choices));
    assert_ne!(first, uuid(&chalk_grids(), &Choices::new().set("title", "Other")));
}

/// What the set represents, when named, is written; nothing is created when
/// nothing is named.
#[test]
fn what_the_set_represents_is_what_is_named() {
    let plan = planned(&bag(&chalk_grids()), &Choices::new().set("title", "Chalk grids")).unwrap();
    assert!(the_set(&plan).get("RepresentedObject").is_none());
    assert_eq!(plan.objects.len(), 1);
    let model = Ref::new("e0de1000-0000-4000-8000-000000000001", "resqml22.EarthModelInterpretation", "Lutetia");
    let plan = planned(&bag(&chalk_grids()), &Choices::new().set("title", "Chalk grids").set("interpretation", model.dor())).unwrap();
    assert_eq!(the_set(&plan)["RepresentedObject"]["Uuid"], json!(model.uuid));
}
