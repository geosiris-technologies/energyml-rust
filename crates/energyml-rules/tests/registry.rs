//! The shipped registry, read through the public API.

use energyml_rules::{
    AnchorKind, Status, all, at, get, id, in_theme, locales, superseded_by, text_in, themes,
};
use std::collections::BTreeSet;

/// Ids are unique, aliases included — the build refuses otherwise, and this
/// states it of what shipped.
///
/// To break it: duplicate a rule in `rules/inherit.toml` (the build refuses,
/// with `DuplicateId`); or, to judge the test alone, duplicate an entry of
/// `RULES` in the generated file.
#[test]
fn ids_are_unique() {
    let mut seen = BTreeSet::new();
    for rule in all() {
        assert!(seen.insert(rule.id), "{} appears twice", rule.id);
        for alias in rule.aliases {
            assert!(seen.insert(alias), "alias {alias} of {} collides", rule.id);
        }
    }
}

/// Every id has the shape the design fixes, the theme field agrees with the
/// id, and the theme exists.
///
/// To break it: write `theme = "colour"` under a `house/inherit/…` id.
#[test]
fn ids_carry_their_theme() {
    let declared: BTreeSet<_> = themes().iter().map(|t| t.id).collect();
    for rule in all() {
        assert!(
            declared.contains(rule.theme),
            "{} names the undeclared theme {}",
            rule.id,
            rule.theme
        );
        let flat = rule.id.matches('/').count() == 1;
        if flat {
            assert!(
                rule.id == id::DISCRETE_VALUE_IN_LOOKUP || rule.id == id::TIME_SERIES_ASSET_RESOLVES,
                "{} is flat and is not one of the two published ids",
                rule.id
            );
        } else {
            assert!(
                rule.id.starts_with(&format!("house/{}/", rule.theme)),
                "{} is not under its theme {}",
                rule.id,
                rule.theme
            );
        }
    }
}

/// The English is present everywhere: title one line, text and reason
/// non-empty. It is the source; the translations are optional.
///
/// To break it: empty a `why` in a `rules/*.toml` (the build refuses); or
/// leave a line break in a `title`.
#[test]
fn english_is_present_everywhere() {
    for rule in all() {
        assert!(
            !rule.title.trim().is_empty() && !rule.title.contains('\n'),
            "{}: the title is one line",
            rule.id
        );
        assert!(!rule.text.trim().is_empty(), "{}: text", rule.id);
        assert!(!rule.why.trim().is_empty(), "{}: why", rule.id);
    }
}

/// The 32 rules (inheritance, colours, attachments) and the two published ids
/// are all in, with the statuses the design gives: the inheritance rules
/// (ADR-0009) were `designed` and are `active` since the walk is coded; the
/// earlier FIRP rule and the colour cascade are `superseded` by `rank-order`,
/// in the same commit.
///
/// To break it: put `house/inherit/firp-parents` back to `active` (the test goes
/// red), or leave it `superseded` without `rank-order` naming it (the build
/// refuses); or remove a rule from a file.
#[test]
fn the_rules_are_complete_and_statuses_follow_the_design() {
    let count = |theme: &str| in_theme(theme).count();
    assert_eq!(
        (count("inherit"), count("colour"), count("attach")),
        (9, 14, 9),
        "the 32 rules of the registry"
    );
    assert_eq!(count("table") + count("series"), 2, "the two published ids");
    assert_eq!(all().len(), 34);

    let coded_by_the_new_walk = [
        id::INHERIT_RANK_ORDER,
        id::INHERIT_SHORTEST_PATH_THEN_DOCUMENT_ORDER,
        id::INHERIT_TIES_ARE_KEPT,
        id::INHERIT_MODEL_COLLECTIONS_NOT_FOLLOWED,
        id::INHERIT_PROPERTY_KIND_ONLY,
        id::INHERIT_COLLECTION_IS_POINTER_ARRAY,
        id::INHERIT_CYCLE_REFUSED_DUPLICATES_ONCE,
    ];
    for rule_id in coded_by_the_new_walk {
        assert_eq!(get(rule_id).unwrap().status, Status::Active, "{rule_id}");
    }
    assert_eq!(
        get(id::INHERIT_FIRP_PARENTS).unwrap().status,
        Status::Superseded,
        "superseded when the rank order was written"
    );
    assert_eq!(
        get("house/colour/specific-wins-with-fallbacks").unwrap().status,
        Status::Superseded,
        "superseded when the rank order was written"
    );
    let replacements: Vec<&str> = superseded_by(id::INHERIT_FIRP_PARENTS).map(|r| r.id).collect();
    assert_eq!(
        replacements,
        vec![id::INHERIT_RANK_ORDER],
        "the history is read beside what replaces it"
    );
    assert_eq!(get(id::DISCRETE_VALUE_IN_LOOKUP).unwrap().status, Status::Active);
    assert_eq!(
        get(id::TIME_SERIES_ASSET_RESOLVES).unwrap().status,
        Status::Active
    );
}

/// History is information: a superseded rule, if there is one, is read beside
/// what replaces it, and the replacement names it. (Two are.)
///
/// To break it: see `loading::a_superseded_rule_is_read_beside_its_replacement`.
#[test]
fn every_superseded_rule_has_a_replacement_that_names_it() {
    for rule in all().iter().filter(|r| r.status == Status::Superseded) {
        assert!(
            superseded_by(rule.id).count() >= 1,
            "{} is superseded by nobody",
            rule.id
        );
    }
    for rule in all() {
        for old in rule.supersedes {
            assert_eq!(
                get(old).map(|r| r.status),
                Some(Status::Superseded),
                "{} supersedes {old}",
                rule.id
            );
        }
    }
}

/// The constants are the ids: `get` answers for each, and a constant is the
/// very string of its rule.
///
/// To break it: change the spelling of an id in a `rules/*.toml`; the constant
/// follows (it is generated), so this test does not go red — the safeguard is
/// that the code citing `id::TYPO` does not **compile** (tried by hand).
#[test]
fn constants_resolve() {
    for (constant, theme) in [
        (id::INHERIT_RANK_ORDER, "inherit"),
        (id::COLOUR_STACK_TOP_WINS, "colour"),
        (id::ATTACH_MD_MEANING, "attach"),
    ] {
        let rule = get(constant).unwrap_or_else(|| panic!("{constant} resolves nothing"));
        assert_eq!(rule.id, constant);
        assert_eq!(rule.theme, theme);
    }
    assert!(get("house/inherit/no-such-rule").is_none());
}

/// The reverse link: *which rules hold here?* — a view and a type, the exact
/// key.
///
/// To break it: remove `{ type = "view", key = "object.colour-stack" }` from
/// `house/colour/stack-top-wins`.
#[test]
fn the_reverse_link_finds_rules_at_an_anchor() {
    let here: Vec<_> = at(AnchorKind::View, "object.colour-stack")
        .map(|r| r.id)
        .collect();
    assert!(here.contains(&id::COLOUR_STACK_TOP_WINS));
    assert!(here.contains(&id::INHERIT_RANK_ORDER));
    assert!(!here.contains(&id::ATTACH_MD_MEANING));
    let on_type: Vec<_> = at(AnchorKind::Type, "Attachment.Md").map(|r| r.id).collect();
    assert_eq!(on_type, [id::ATTACH_MD_MEANING]);
    assert_eq!(at(AnchorKind::View, "object.no-such-view").count(), 0);
    // The kind matters: the same key under another kind is another place.
    assert_eq!(at(AnchorKind::Type, "object.colour-stack").count(), 0);
}

/// French is optional **per rule**, and a rule not translated answers in
/// English and says so — a missing word never passes for a language.
///
/// To break it: make `text_in` return `fallback: false` when nothing is
/// translated.
#[cfg(feature = "fr")]
#[test]
fn a_missing_translation_falls_back_to_english_and_says_so() {
    assert!(locales().contains(&"fr"));
    // Title and text translated, reason not: English there, and the flag says so.
    let rank = text_in(get(id::INHERIT_RANK_ORDER).unwrap(), "fr");
    assert_eq!(rank.title.lang, "fr");
    assert_eq!(rank.text.lang, "fr");
    assert_eq!(rank.why.lang, "en");
    assert!(rank.fallback);
    // Title only.
    let tie = text_in(get(id::INHERIT_TIES_ARE_KEPT).unwrap(), "fr");
    assert_eq!((tie.title.lang, tie.text.lang), ("fr", "en"));
    assert_ne!(tie.title.value, get(id::INHERIT_TIES_ARE_KEPT).unwrap().title);
    // Nothing: all English, flagged.
    let none = text_in(get(id::COLOUR_STACK_TOP_WINS).unwrap(), "fr");
    assert!(none.fallback);
    assert_eq!(none.title.value, get(id::COLOUR_STACK_TOP_WINS).unwrap().title);
    // English asked: never a fallback; an unknown language falls back too.
    assert!(!text_in(get(id::INHERIT_RANK_ORDER).unwrap(), "en").fallback);
    assert!(text_in(get(id::INHERIT_RANK_ORDER).unwrap(), "de").fallback);
}

/// The JSON the crate writes by hand is read back by an independent parser and
/// agrees with the registry: same rules in the same order, every text intact
/// (quotes, accents), French present only where translated.
///
/// To break it: remove the `"` escape in `select::esc`; the JSON can no longer
/// be read.
#[test]
fn the_json_agrees_with_the_registry() {
    let rules: Vec<_> = all().iter().collect();
    let json = energyml_rules::select::to_json(&rules);
    let doc: serde_json::Value = serde_json::from_str(&json).expect("the hand-written JSON is JSON");
    assert_eq!(doc["version"], energyml_rules::RULESET_VERSION);
    let written = doc["rules"].as_array().unwrap();
    assert_eq!(written.len(), all().len());
    for (rule, value) in all().iter().zip(written) {
        assert_eq!(value["id"], rule.id);
        assert_eq!(value["status"], rule.status.as_str());
        assert_eq!(
            value["text"]["en"], rule.text,
            "{}: text survives the escaping",
            rule.id
        );
        assert_eq!(value["refs"].as_array().unwrap().len(), rule.refs.len());
        assert_eq!(value["rulings"].as_array().unwrap().len(), rule.rulings.len());
    }
    #[cfg(feature = "fr")]
    {
        let rank = written
            .iter()
            .find(|v| v["id"] == id::INHERIT_RANK_ORDER)
            .unwrap();
        assert!(rank["title"]["fr"].is_string());
        assert!(
            rank["why"].get("fr").is_none(),
            "French appears only where translated"
        );
        let top = written
            .iter()
            .find(|v| v["id"] == id::COLOUR_STACK_TOP_WINS)
            .unwrap();
        assert!(top["title"].get("fr").is_none());
    }
}
