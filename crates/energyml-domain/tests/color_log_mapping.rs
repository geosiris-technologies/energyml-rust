//! `UseLogarithmicMapping` is the log of the value - `ColorMap::try_sample`.
//!
//! RESQML TRG 2.2 §4.5 ColorInformation p. 56: `UseLogarithmicMapping`
//! *"Indicates that the log of the property values are taken into account when
//! mapped with the index of the color map"*; `MinMax` is the range of the
//! values that give the minimum and the maximum colour. §4.7 ContinuousColorMap
//! p. 58 and §4.23 p. 74: `InterpolationMethod` (`linear`, `logarithmic`) is
//! the spacing **between two stops**, a different thing.
//!
//! Maps are built in memory; nothing here reads a file.

use energyml_domain::color::{ColorMap, Domain, Hsv, Interpolation, Rgba, Scaling, Stop};

const TOLERANCE: f64 = 1e-3;

/// A fully saturated colour of hue `hue` (red 0, green 120, blue 240).
fn hsv(hue: f64) -> Hsv {
    Hsv {
        hue,
        saturation: 1.0,
        value: 1.0,
        alpha: 1.0,
        title: None,
    }
}

/// A continuous map in RGB, with the given stops (index, hue) and spacing.
fn map(stops: &[(f64, f64)], method: Interpolation) -> ColorMap {
    ColorMap {
        uuid: "00000000-0000-0000-0000-00000000c010".to_string(),
        title: None,
        continuous: Some((Domain::Rgb, method)),
        stops: stops
            .iter()
            .map(|(index, hue)| Stop {
                index: *index,
                color: hsv(*hue),
            })
            .collect(),
        null_color: None,
        above_max: None,
        below_min: None,
    }
}

fn log_over(min: f64, max: f64) -> Scaling {
    Scaling {
        min_max: Some((min, max)),
        logarithmic: true,
        reverse: false,
    }
}

fn log_alone() -> Scaling {
    Scaling {
        min_max: None,
        logarithmic: true,
        reverse: false,
    }
}

fn near(got: Option<Rgba>, want: (f64, f64, f64), what: &str) {
    let got = got.unwrap_or_else(|| panic!("{what}: no colour"));
    assert!(
        (got.r - want.0).abs() < TOLERANCE && (got.g - want.1).abs() < TOLERANCE && (got.b - want.2).abs() < TOLERANCE,
        "{what}: got ({}, {}, {}), wanted {want:?}",
        got.r,
        got.g,
        got.b
    );
}

const RED: (f64, f64, f64) = (1.0, 0.0, 0.0);
const BLUE: (f64, f64, f64) = (0.0, 0.0, 1.0);

/// TRG 2.2 §4.5 p. 56. To break it: ignore `scaling.logarithmic` in the placement
/// of `try_sample`.
#[test]
fn the_log_of_the_value_is_placed_between_min_and_max() {
    let map = map(&[(0.0, 0.0), (1.0, 240.0)], Interpolation::Linear);
    let scaling = log_over(1.0, 100.0);
    near(map.sample(Some(10.0), &scaling), (0.5, 0.0, 0.5), "10 is half way in log");
    near(map.sample(Some(1.0), &scaling), RED, "the minimum");
    near(map.sample(Some(100.0), &scaling), BLUE, "the maximum");
    near(map.sample(Some(0.0), &scaling), RED, "zero takes the extreme colour");
    near(map.sample(Some(-5.0), &scaling), RED, "a negative takes the extreme colour");

    // The control: the same value, placed linearly.
    let linear = Scaling {
        min_max: Some((1.0, 100.0)),
        logarithmic: false,
        reverse: false,
    };
    near(map.sample(Some(10.0), &linear), (0.909, 0.0, 0.091), "10 placed linearly");
}

/// The case of D5: a map whose first index is 0 was flattened by the old code.
/// To break it: put back `|| scaling.logarithmic` in `continuous_at` and the
/// linear placement.
#[test]
fn a_map_starting_at_zero_is_not_flattened() {
    let map = map(&[(0.0, 0.0), (50.0, 120.0), (100.0, 240.0)], Interpolation::Linear);
    // log10 of 10 over [1, 1000] is 1/3: position 100/3, window [0, 50], t = 2/3.
    near(
        map.sample(Some(10.0), &log_over(1.0, 1000.0)),
        (0.333, 0.667, 0.0),
        "a third of the way in log",
    );
}

/// TRG 2.2 §4.5 against §4.7: two distinct things. To break it: write again
/// "both mean the same thing here" in `continuous_at`.
#[test]
fn the_map_range_stands_for_the_values_without_min_max() {
    let linear_map = map(&[(1.0, 0.0), (10.0, 120.0), (100.0, 240.0)], Interpolation::Linear);
    let root = 10f64.sqrt();
    // Position 1 + 0.25 * 99 = 25.75, window [10, 100], t = 0.175.
    near(
        linear_map.sample(Some(root), &log_alone()),
        (0.0, 0.825, 0.175),
        "log of the value over the map's own range",
    );

    // `InterpolationMethod = logarithmic` alone: the value is placed as is, the
    // spacing between the two stops [1, 10] is logarithmic, t = 0.5.
    let log_spaced = map(&[(1.0, 0.0), (10.0, 120.0), (100.0, 240.0)], Interpolation::Logarithmic);
    near(
        log_spaced.sample(Some(root), &Scaling::default()),
        (0.5, 0.5, 0.0),
        "logarithmic spacing between two stops",
    );
}

/// A range that reaches zero has no logarithm: refused by name, once.
/// To break it: let `check_scaling` answer `Ok(())`.
#[test]
fn a_logarithmic_mapping_needs_values_with_a_logarithm() {
    let map = map(&[(0.0, 0.0), (1.0, 240.0)], Interpolation::Linear);

    let scaling = log_alone();
    let refusal = map.check_scaling(&scaling).expect_err("a map from 0, no MinMax");
    let text = refusal.to_string();
    assert!(text.contains("UseLogarithmicMapping") && text.contains('0'), "{text}");
    assert!(map.try_sample(Some(0.5), &scaling).is_err());
    assert_eq!(map.sample(Some(0.5), &scaling), None);

    let scaling = log_over(0.0, 10.0);
    assert!(map.check_scaling(&scaling).is_err(), "a MinMax that reaches 0");
    assert!(map.try_sample(Some(0.5), &scaling).is_err());

    // Not logarithmic: nothing to refuse.
    assert!(map.check_scaling(&Scaling::default()).is_ok());
}

/// A null value has its colour whatever the scale. To break it: call
/// `check_scaling` before the null test in `try_sample`.
#[test]
fn a_null_value_keeps_its_null_colour_under_a_refused_log_mapping() {
    let mut map = map(&[(0.0, 0.0), (1.0, 240.0)], Interpolation::Linear);
    map.null_color = Some(hsv(60.0));
    let yellow = hsv(60.0).to_rgba();

    let got = map.try_sample(None, &log_alone()).expect("a null is never refused");
    assert_eq!(got, Some(yellow));
    let got = map.try_sample(Some(f64::NAN), &log_alone()).expect("NaN is a null");
    assert_eq!(got, Some(yellow));
}
