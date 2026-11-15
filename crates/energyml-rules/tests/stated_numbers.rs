//! The numbers a declared rule states are the code's constants: two sources
//! of one fact, the registry's text and the domain's code (the shape of
//! `crosscheck.rs`). A constant that moves without its rule, or a rule
//! rewritten without its constant, goes red here.

use energyml_domain::along_md::{GAP_RATIO, JUMP_RATIO};
use energyml_domain::stats::quantile::EXACT_QUANTILE_LIMIT;
use energyml_domain::stats::scale::{AUTO_HIGH, AUTO_LOW, LOG_DECADES};
use energyml_domain::stats::sketch::SKETCH_RELATIVE_ERROR;
use energyml_rules::{get, id};

/// The title and the text of a rule, as one string to search.
fn said(rule_id: &str) -> String {
    let rule = get(rule_id).unwrap_or_else(|| panic!("{rule_id} is in the registry"));
    format!("{} {}", rule.title, rule.text)
}

/// To break it: set `GAP_RATIO` to 2.0, or `LOG_DECADES` to 4.0, in the
/// domain; or write "4 times" in `rules/stats.toml`.
#[test]
fn the_numbers_of_the_stats_rules_are_the_codes() {
    assert!(said(id::STATS_GAP_NEVER_BRIDGED).contains(&format!("{GAP_RATIO} times the median step")));
    assert!(said(id::STATS_SEAM_IS_SIGNALLED).contains(&format!("{JUMP_RATIO} times the median step")));
    assert!(said(id::STATS_LOG_FROM_THREE_DECADES).contains(&format!("at least {LOG_DECADES} decades")));
    let bounds = said(id::STATS_AUTOMATIC_BOUNDS_ARE_PERCENTILES);
    let (low, high) = ((AUTO_LOW * 100.0).round(), (AUTO_HIGH * 100.0).round());
    assert!(
        bounds.contains(&format!("{low}st and {high}th percentiles")),
        "{bounds}"
    );
    assert_eq!(EXACT_QUANTILE_LIMIT % 1_000_000, 0, "the text counts in millions");
    assert!(bounds.contains(&format!("{} million values", EXACT_QUANTILE_LIMIT / 1_000_000)));
    assert!(bounds.contains(&format!("{} %", (SKETCH_RELATIVE_ERROR * 100.0).round())));
}
