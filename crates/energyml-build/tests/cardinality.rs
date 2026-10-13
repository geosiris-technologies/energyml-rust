//! A slot that takes several values, end to end.
//!
//! # Why this file exists before the operation that needs it
//!
//! `Slot` carries `least` and `most` — a `MinOccurs` and a `MaxOccurs` — **since
//! the first day**, and none of the thirteen operations uses them: all are
//! `1..1`. Cardinality was therefore declared and never exercised, which is the
//! form of debt that the project's contribution rules name — *a capability that nothing
//! proves*.
//!
//! The risk was not the field, it was that the whole chain below it
//! silently assumes a single value. This file crosses it once,
//! with an operation declared **here** rather than shipped: a fake operation
//! in the registry would be an entry that the front end would offer to someone.
//!
//! The declaration of cardinality is wired in
//! *before* the operation that demands it, otherwise it is the second novelty that
//! would explain the failure of the first.
//!
//! # What would have to break to make it turn red
//!
//! Reading the first value of a list instead of all; accepting a bare
//! value where a list is expected; checking only the first element;
//! forgetting a bound. Each has its test, and each was verified by breaking it.

use serde_json::{Value, json};

use energyml_build::operation::{AnyOperation, Choices, Error, Fitness, Identity, Need, Plan, Slot, When};
use energyml_build::pieces::Pieces;

fn when() -> When {
    When {
        creation: "2026-09-11T00:00:00Z".into(),
        originator: "test".into(),
    }
}

/// An operation that takes **as many titles as you like**, and names them all.
///
/// The smallest possible subject: what is checked is cardinality, not
/// geometry. A `Need::Title` because it accepts a string and nothing else,
/// so an invalid element shows.
struct ManyTitles {
    least: u32,
    most: Option<u32>,
}

impl AnyOperation for ManyTitles {
    fn name(&self) -> &'static str {
        "many-titles"
    }
    fn describe(&self) -> &'static str {
        "takes several titles and names them all"
    }
    fn makes(&self) -> &'static [&'static str] {
        &[]
    }
    fn recipe(&self) -> &'static str {
        "many-titles"
    }
    fn fitness(&self, _pieces: &Pieces) -> Fitness {
        Fitness::Fit
    }
    fn needs(&self, _pieces: &Pieces) -> Vec<Slot> {
        let mut slot = Slot::many("titles", Need::Title, "the titles to name", self.least);
        slot.most = self.most;
        vec![slot]
    }
    fn plan(
        &self,
        _pieces: &Pieces,
        choices: &Choices,
        _identity: &Identity,
        _when: &When,
    ) -> Result<Plan, Error> {
        // **All of them**, not the first. This is the line that this file watches:
        // a consumer that read `choices.text("titles")` would take `None`
        // on a list and say nothing.
        let given = choices
            .get("titles")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let names: Vec<String> = given
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        Ok(Plan {
            media: Vec::new(),
            blocked: None,
            objects: Vec::new(),
            datasets: Vec::new(),
            activity: energyml_build::operation::ActivityDraft {
                recipe: "many-titles",
                title: names.join(" + "),
                seed: names.join("|"),
                parameters: Vec::new(),
            },
            says: vec![energyml_build::operation::Note {
                key: "named",
                detail: format!("{} title(s): {}", names.len(), names.join(", ")),
            }],
            dropped: Vec::new(),
            // A test operation: it checks cardinality, it computes no
            // geometry.
            shapes: Vec::new(),
        })
    }
}

fn settle(op: &ManyTitles, choices: Choices) -> Result<Choices, Error> {
    let pieces = Pieces::empty();
    choices.settle(&op.needs(&pieces))
}

fn free() -> ManyTitles {
    ManyTitles { least: 1, most: None }
}

// ---------------------------------------------------------------------------
// The shape
// ---------------------------------------------------------------------------

#[test]
fn a_multiple_slot_is_declared_as_such() {
    let op = free();
    let slot = &op.needs(&Pieces::empty())[0];
    assert!(slot.takes_many());
    assert_eq!(slot.least, 1);
    assert_eq!(slot.most, None);

    // And says so to the front end, rather than letting it rederive the rule.
    assert_eq!(slot.json()["takesMany"], json!(true));
    assert_eq!(slot.json()["most"], Value::Null);
}

/// The case that tells them apart: `0..1` **is not** a list.
///
/// *May be absent* and *may hold one* are the same shape, and
/// `Slot::optional` has always meant the first. Without this assertion, one
/// day somebody will make `least == 0` the criterion and all the optional fields
/// of the front end will become lists of one element.
/// The order is **declared**, never deduced from the rest.
///
/// The reason is in a front end's ordering code: the order of the
/// surfaces of a section is *the most dangerous datum of the whole section*,
/// since getting it reversed looks abnormal to nobody. A
/// form that guessed it would be deciding that.
#[test]
fn the_order_is_stated_and_not_deduced() {
    let plain = Slot::many("surfaces", Need::Title, "", 2);
    assert!(plain.takes_many());
    assert!(
        !plain.ordered,
        "taking several values says nothing of their order"
    );
    assert_eq!(plain.json()["ordered"], json!(false));

    let ranked = Slot::many("surfaces", Need::Title, "", 2).in_order();
    assert!(ranked.ordered);
    assert_eq!(ranked.json()["ordered"], json!(true));
}

/// And the order is not put on a slot that has only one value: there is no
/// order of one, and setting it would put in the template a flag that a
/// form would then have to decide to ignore.
#[test]
fn a_single_slot_has_no_order() {
    let slot = Slot::optional("ground", Need::Title, "");
    assert!(!slot.ordered);
    assert_eq!(slot.json()["ordered"], json!(false));
}

#[test]
fn an_optional_slot_stays_a_value() {
    let slot = Slot::optional("ground", Need::Title, "");
    assert!(!slot.takes_many());
    assert_eq!(slot.json()["takesMany"], json!(false));
}

#[test]
fn a_bare_value_in_a_multiple_slot_is_refused_by_its_shape() {
    let why = settle(&free(), Choices::new().set("titles", "seul")).unwrap_err();
    // The refusal names the expected shape **and** what arrived: saying
    // “0 values, at least 1” would send one looking for an answer that was given.
    assert!(why.to_string().contains("list"), "{why}");
    assert!(why.to_string().contains("text"), "{why}");
}

// ---------------------------------------------------------------------------
// The bounds
// ---------------------------------------------------------------------------

#[test]
fn too_few_values_is_refused_with_the_count() {
    let op = ManyTitles { least: 2, most: None };
    let why = settle(&op, Choices::new().set("titles", json!(["un"]))).unwrap_err();
    assert!(why.to_string().contains("at least 2"), "{why}");
    assert!(why.to_string().contains("has 1"), "{why}");
}

#[test]
fn too_many_values_is_refused_with_the_count() {
    let op = ManyTitles {
        least: 0,
        most: Some(2),
    };
    let why = settle(&op, Choices::new().set("titles", json!(["a", "b", "c"]))).unwrap_err();
    assert!(why.to_string().contains("at most 2"), "{why}");
    assert!(why.to_string().contains("has 3"), "{why}");
}

#[test]
fn an_empty_list_is_an_absence() {
    // Telling them apart would give two ways of saying *nothing*, and the front end would
    // pick one at random.
    let op = ManyTitles { least: 0, most: None };
    assert!(settle(&op, Choices::new().set("titles", json!([]))).is_ok());
    assert!(settle(&op, Choices::new()).is_ok());

    // And when at least one is needed, both refuse — by the same path
    // as any slot required for the second.
    let strict = free();
    assert!(settle(&strict, Choices::new().set("titles", json!([]))).is_err());
    assert!(settle(&strict, Choices::new()).is_err());
}

// ---------------------------------------------------------------------------
// The elements
// ---------------------------------------------------------------------------

#[test]
fn every_element_is_checked_not_only_the_first() {
    let why = settle(&free(), Choices::new().set("titles", json!(["bon", 42, true]))).unwrap_err();
    // The two bad ones are named, with their rank: correcting a list must not
    // cost as many round trips as it has wrong entries.
    assert!(why.to_string().contains("titles`[1]"), "{why}");
    assert!(why.to_string().contains("titles`[2]"), "{why}");
    assert!(!why.to_string().contains("titles`[0]"), "{why}");
}

// ---------------------------------------------------------------------------
// The whole chain
// ---------------------------------------------------------------------------

#[test]
fn the_plan_sees_all_the_values_and_not_the_first() {
    let op = free();
    let choices = settle(
        &op,
        Choices::new().set("titles", json!(["Top Brent", "Base Brent", "Ness"])),
    )
    .unwrap();
    let plan = op
        .plan(&Pieces::empty(), &choices, &Identity::derived(), &when())
        .unwrap();

    assert_eq!(plan.activity.title, "Top Brent + Base Brent + Ness");
    let note = plan.says.iter().find(|n| n.key == "named").expect("the note");
    assert!(note.detail.starts_with("3 title(s)"), "{}", note.detail);
    assert!(note.detail.contains("Ness"), "{}", note.detail);
}

/// The negative check of the whole file.
///
/// Without it, a `settle` that let anything through would make the tests
/// above green for the wrong reason: they mostly verify refusals,
/// and a refusal that never arrives is only seen through what follows.
#[test]
fn a_right_list_passes_intact() {
    let out = settle(&free(), Choices::new().set("titles", json!(["a", "b"]))).unwrap();
    assert_eq!(out.get("titles"), Some(&json!(["a", "b"])));
}
