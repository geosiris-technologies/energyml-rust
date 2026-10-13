//! `colormap` told its bands in words - `levels` and `colours` - for a caller
//! with no ramp to hand: a recipe. The need (06/10/2026), to test that a painting
//! follows its colour map: a map on the dip of Lutetia's DTM, written by the
//! recipe. One or the other: a ramp **and** levels, or neither, is refused by
//! name.

use energyml_build::operation::{Choices, Identity, Registry, When};
use energyml_build::pieces::{Pieces, Source};
use energyml_domain::color::Hsv;

fn when() -> When {
    When {
        creation: "2026-10-06T00:00:00Z".into(),
        originator: "test".into(),
    }
}

fn words() -> Choices {
    Choices::new()
        .set("title", "dip")
        .set("levels", serde_json::json!([0.0, 2.0, 5.0]))
        .set("colours", serde_json::json!(["#0000ff", "#00ff00", "#ff0000"]))
}

#[test]
fn levels_and_colours_said_in_words_make_the_map_a_ramp_would() {
    let plan = Registry::new()
        .plan(
            "colormap",
            &Pieces::empty(),
            &words(),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|why| panic!("{why}"));
    let map = plan
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.ContinuousColorMap")
        .expect("a map");
    let entries = map.json["Entry"].as_array().expect("entries");
    // Flat bands: two stops per band but the last, which runs upwards.
    assert!(entries.len() >= 5, "{entries:?}");
    let hue = |at: usize| entries[at]["Hsv"]["Hue"].as_f64().unwrap();
    assert_eq!(hue(0), 240.0, "the first band is the first colour, blue");
    assert_eq!(
        hue(entries.len() - 1),
        0.0,
        "the last band is the last colour, red"
    );

    // The same bands through a ramp give the same entries.
    let colour = |text: &str| Hsv::from_rgba(energyml_domain::color::Rgba::parse(text).unwrap());
    let bag = Pieces::empty().with(
        "ramp",
        Source::Ramp {
            levels: vec![0.0, 2.0, 5.0],
            colours: vec![colour("#0000ff"), colour("#00ff00"), colour("#ff0000")],
        },
    );
    let ramped = Registry::new()
        .plan(
            "colormap",
            &bag,
            &Choices::new().set("title", "dip"),
            &Identity::derived(),
            &when(),
        )
        .unwrap_or_else(|why| panic!("{why}"));
    let other = ramped
        .objects
        .iter()
        .find(|one| one.qualified_type == "resqml22.ContinuousColorMap")
        .unwrap();
    assert_eq!(
        other.json["Entry"], map.json["Entry"],
        "one map, whichever way its bands are said"
    );
}

#[test]
fn a_ramp_and_levels_together_or_neither_is_refused_by_name() {
    let bag = Pieces::empty().with(
        "ramp",
        Source::Ramp {
            levels: vec![0.0, 1.0],
            colours: vec![Hsv::from_rgba(energyml_domain::color::Rgba::parse("#000000").unwrap()); 2],
        },
    );
    let both = Registry::new()
        .plan("colormap", &bag, &words(), &Identity::derived(), &when())
        .expect_err("two answers");
    assert!(both.message().contains("both given"), "{both}");
    let neither = Registry::new()
        .plan(
            "colormap",
            &Pieces::empty(),
            &Choices::new().set("title", "dip"),
            &Identity::derived(),
            &when(),
        )
        .expect_err("no bands");
    assert!(neither.message().contains("needs its bands"), "{neither}");
    let unequal = Registry::new()
        .plan(
            "colormap",
            &Pieces::empty(),
            &words().set("colours", serde_json::json!(["#0000ff"])),
            &Identity::derived(),
            &when(),
        )
        .expect_err("three levels, one colour");
    assert!(unequal.message().contains("one colour per band"), "{unequal}");
}
