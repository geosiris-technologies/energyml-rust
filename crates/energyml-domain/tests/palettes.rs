//! The palettes offered when the file says no colour map: quoted colours, one rule of sampling, the caller's first.

use energyml_domain::color::{Rgba, Scaling};
use energyml_domain::palettes::{BUILT_IN, Palette, Palettes, Reading, Source};

fn bytes(rgba: Rgba) -> [u8; 3] {
    let channel = |value: f64| (value * 255.0).round() as u8;
    [channel(rgba.r), channel(rgba.g), channel(rgba.b)]
}

fn over(low: f64, high: f64) -> Scaling {
    Scaling {
        min_max: Some((low, high)),
        logarithmic: false,
        reverse: false,
    }
}

#[test]
fn a_ramp_gives_back_the_quoted_colours_at_its_stops() {
    // Each built-in ramp, sampled at each of its stops, is the colour its source publishes there.
    for palette in BUILT_IN
        .iter()
        .filter(|palette| palette.reading == Reading::Continuous)
    {
        let map = palette.ramp();
        let last = (palette.colours.len() - 1) as f64;
        for (at, quoted) in palette.colours.iter().enumerate() {
            let sampled = map.sample(Some(at as f64), &over(0.0, last)).expect("a colour");
            assert_eq!(bytes(sampled), *quoted, "{} at stop {at}", palette.name);
        }
    }
    // Break it: space the stops by the colour's index over the count rather than over the count less one.
}

#[test]
fn the_defaults_are_viridis_for_a_value_and_set2_for_a_category() {
    let palettes = Palettes::new();
    assert_eq!(
        palettes.default_for(Reading::Continuous).map(|p| p.name),
        Some("Viridis")
    );
    assert_eq!(
        palettes.default_for(Reading::Categorical).map(|p| p.name),
        Some("Set2")
    );
    assert!(
        palettes
            .for_reading(Reading::Continuous)
            .iter()
            .all(|p| p.reading == Reading::Continuous)
    );
    assert_eq!(Source::Matplotlib.licence(), "CC0-1.0");
    // Break it: put Spectral first in the list, and the default changes.
}

#[test]
fn a_callers_palette_is_tried_first_and_replaces_a_built_in_of_its_name() {
    let mut palettes = Palettes::new();
    palettes.register(Palette {
        name: "Viridis",
        source: Source::ColorBrewer,
        reading: Reading::Continuous,
        colours: &[[0, 0, 0], [255, 255, 255]],
    });
    assert_eq!(
        palettes.named("viridis").map(|p| p.colours.len()),
        Some(2),
        "the caller's, found without case"
    );
    assert_eq!(
        palettes.all().iter().filter(|p| p.name == "Viridis").count(),
        1,
        "one per name"
    );
    // Break it: append the caller's palette after the built-ins.
}

#[test]
fn a_value_no_category_names_takes_no_colour() {
    // Keys 1, 2 and 5: a value 3 names no row, and the nearest lower entry would paint it in the colour of 2.
    let map = BUILT_IN[3].over_categories(&[1.0, 2.0, 5.0]).expect("a map");
    let none = Scaling {
        min_max: None,
        logarithmic: false,
        reverse: false,
    };
    assert_eq!(
        map.sample(Some(3.0), &none).map(|c| c.a),
        Some(0.0),
        "transparent between 2 and 5"
    );
    assert_eq!(
        map.sample(Some(5.0), &none).map(bytes),
        Some(BUILT_IN[3].category(2)),
        "5 is the third key"
    );
    // Break it: drop the transparent stop after a key whose successor is not the next integer.
}
