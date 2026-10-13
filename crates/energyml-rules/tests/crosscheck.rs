//! The registry against `energyml-domain::integrity::CHECKS`, **both ways**:
//! two sources of the same fact (the shape of `crossaudit.rs`). `integrity.rs`
//! is read through its public `checks()`, never edited.

use energyml_domain::integrity::checks;
use energyml_rules::{AnchorKind, all, get};
use std::collections::BTreeSet;

/// Every `house/` check of the domain is a rule of the registry, anchored on
/// itself as a `finding`.
///
/// To break it: rename an id in `integrity.rs` (or add a `house/…` check
/// without a rule); the test goes red.
#[test]
fn every_house_check_is_in_the_registry() {
    let houses: Vec<_> = checks().iter().filter(|c| c.id.starts_with("house/")).collect();
    assert!(!houses.is_empty(), "a vacuous cross-check proves nothing: integrity.rs has house/ checks");
    for check in houses {
        let rule = get(check.id).unwrap_or_else(|| panic!("check {} has no rule in the registry", check.id));
        assert!(
            rule.at.iter().any(|a| a.kind == AnchorKind::Finding && a.key == check.id),
            "rule {} must anchor itself as a finding",
            rule.id
        );
        assert!(!check.says.is_empty());
    }
}

/// And back: every rule that declares a finding anchor on a `house/` id names
/// a check that exists.
///
/// To break it: add `{ type = "finding", key = "house/nowhere" }` to a rule.
#[test]
fn every_finding_anchor_names_a_check_that_exists() {
    let known: BTreeSet<_> = checks().iter().map(|c| c.id).collect();
    let mut seen = 0;
    for rule in all() {
        for anchor in rule.at.iter().filter(|a| a.kind == AnchorKind::Finding) {
            seen += 1;
            assert!(known.contains(anchor.key), "rule {} anchors on the unknown check {}", rule.id, anchor.key);
        }
    }
    assert_eq!(seen, 2, "the two published ids");
}
