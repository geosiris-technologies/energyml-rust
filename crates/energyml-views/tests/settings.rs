//! The settings a view declares, checked by the door: names, natures, bounds, defaults.

use std::collections::BTreeMap;

use energyml_views::{SettingKind, Settled, ViewRefusal, ViewSetting};
use serde_json::{Value, json};

fn declared() -> Vec<ViewSetting> {
    vec![
        ViewSetting::choice("mode", &["a", "b"], "a mode").default(json!("a")),
        ViewSetting::number("md", "a depth")
            .uom("m")
            .bounds(Some(0.0), Some(100.0)),
        ViewSetting::integer("count", "a count").bounds(Some(1.0), Some(5.0)),
        ViewSetting::flag("rings", "rings or not"),
        ViewSetting::number("needed", "what must be given").required(),
    ]
}

fn given(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect()
}

#[test]
fn an_unknown_setting_is_refused_by_its_name_with_the_known_ones() {
    let refused = Settled::settle(
        "v",
        &declared(),
        &given(&[("needed", json!(1)), ("colour", json!("red"))]),
    )
    .expect_err("refused");
    let ViewRefusal::UnknownSetting { name, known, .. } = refused else {
        panic!("{refused}")
    };
    assert_eq!(name, "colour");
    assert!(known.contains("mode") && known.contains("md"), "{known}");
    // Break it: let `settle` skip a name it does not declare.
}

#[test]
fn a_value_out_of_its_bounds_or_of_another_nature_is_refused() {
    for (name, value) in [
        ("md", json!(150.0)),
        ("md", json!("deep")),
        ("count", json!(2.5)),
        ("mode", json!("c")),
        ("rings", json!(1)),
    ] {
        let refused = Settled::settle(
            "v",
            &declared(),
            &given(&[("needed", json!(1)), (name, value.clone())]),
        );
        assert!(
            matches!(refused, Err(ViewRefusal::BadSetting { name: ref got, .. }) if got == name),
            "{name} = {value}: {refused:?}"
        );
    }
    // Break it: drop the bounds test of `ViewSetting::check`, and 150 m passes.
}

#[test]
fn the_defaults_are_filled_and_a_whole_number_is_normalised() {
    let settled = Settled::settle(
        "v",
        &declared(),
        &given(&[("needed", json!(1)), ("count", json!(3.0))]),
    )
    .expect("settled");
    assert_eq!(settled.text("mode"), Some("a"), "the default of a choice");
    assert!(!settled.flag("rings"), "a flag is false by default");
    assert_eq!(settled.integer("count"), Some(3), "3.0 is the whole number 3");
    assert_eq!(settled.get("md"), None, "no default, not given: absent");
    // Break it: forget the defaults in `settle`, and `mode` is absent.
}

#[test]
fn a_required_setting_not_given_is_refused() {
    let refused = Settled::settle("v", &declared(), &BTreeMap::new()).expect_err("refused");
    assert!(
        matches!(refused, ViewRefusal::MissingSetting { ref name, .. } if name == "needed"),
        "{refused}"
    );
    // Break it: skip the `required` test, and the setting is silently absent.
}

#[test]
fn a_command_line_text_is_read_by_the_setting_nature() {
    let setting = |name: &str| declared().into_iter().find(|one| one.name == name).unwrap();
    assert_eq!(setting("md").parse("42.5"), Ok(json!(42.5)));
    assert_eq!(setting("rings").parse("yes"), Ok(json!(true)));
    assert_eq!(setting("mode").parse("b"), Ok(json!("b")));
    assert!(
        setting("md").parse("200").is_err(),
        "the bounds hold for a text too"
    );
    let pick = ViewSetting::json("pick", "a pick");
    assert_eq!(pick.parse("[0, 2]"), Ok(json!([0, 2])));
    assert!(matches!(pick.kind, SettingKind::Json));
    // Break it: read every text as a string, and `42.5` is not a number.
}
