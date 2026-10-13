//! `inputs::validate` — every refusal names the field and the value
//! (ADR-0015).

use energyml_section::inputs::{InputsError, MAX_CROSSES, validate};
use energyml_section::model::{CrossOwn, CrossWidths, Crosses, Inputs};

fn widths() -> CrossWidths {
    CrossWidths { before: 40.0, after: 40.0, left: 5.0, right: 5.0 }
}

fn base() -> Inputs {
    Inputs {
        trace: vec![[0.0, 0.0], [100.0, 0.0], [100.0, 50.0]],
        system: None,
        left: 10.0,
        right: 10.0,
        crosses: Crosses::None,
        objects: Vec::new(),
        time_factor: None,
    }
}

fn refused(inputs: &Inputs) -> String {
    validate(inputs).expect_err("must be refused").to_string()
}

#[test]
fn a_good_set_of_inputs_passes() {
    assert_eq!(validate(&base()), Ok(()));
    let mut i = base();
    i.crosses = Crosses::Every { spacing: 25.0, common: widths(), own: Vec::new() };
    i.time_factor = Some(0.5);
    assert_eq!(validate(&i), Ok(()));
}

#[test]
fn a_trace_needs_two_distinct_finite_points() {
    let mut i = base();
    i.trace = vec![[0.0, 0.0]];
    assert!(matches!(validate(&i), Err(InputsError::Trace(_))));
    assert!(refused(&i).contains("at least two points, got 1"));

    i.trace = vec![[0.0, 0.0], [f64::NAN, 1.0]];
    let said = refused(&i);
    assert!(said.contains("point 1") && said.contains("NaN"), "{said}");

    i.trace = vec![[0.0, 0.0], [5.0, 5.0], [5.0, 5.0]];
    let said = refused(&i);
    assert!(said.contains("points 1 and 2 coincide"), "{said}");

    i.trace = vec![[0.0, 0.0], [f64::INFINITY, 1.0]];
    assert!(matches!(validate(&i), Err(InputsError::Trace(_))));
}

#[test]
fn a_width_is_finite_and_not_negative_and_one_side_at_least_has_one() {
    let mut i = base();
    i.left = -3.0;
    let said = refused(&i);
    assert!(said.contains("left width") && said.contains("-3"), "{said}");

    let mut i = base();
    i.right = f64::NAN;
    assert!(refused(&i).contains("right width"));

    // Zero on one side is fine (a one-sided corridor); on both it keeps nothing.
    let mut i = base();
    i.right = 0.0;
    assert_eq!(validate(&i), Ok(()));
    i.left = 0.0;
    assert!(refused(&i).contains("both 0"));
}

#[test]
fn the_time_factor_is_above_zero_when_given() {
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut i = base();
        i.time_factor = Some(bad);
        assert!(refused(&i).contains("time factor"), "{bad}");
    }
}

#[test]
fn the_spacing_and_the_count_are_usable() {
    for bad in [0.0, -5.0, f64::NAN] {
        let mut i = base();
        i.crosses = Crosses::Every { spacing: bad, common: widths(), own: Vec::new() };
        let said = refused(&i);
        assert!(said.contains("spacing"), "{said}");
    }
    let mut i = base();
    i.crosses = Crosses::Count { count: 0, common: widths(), own: Vec::new() };
    assert!(refused(&i).contains("at least 1, got 0"));
    i.crosses = Crosses::Count { count: MAX_CROSSES as u32 + 1, common: widths(), own: Vec::new() };
    assert!(refused(&i).contains("limit"));
}

#[test]
fn the_widths_of_a_cross_are_checked_common_and_own() {
    let mut i = base();
    let mut w = widths();
    w.before = -1.0;
    i.crosses = Crosses::Every { spacing: 25.0, common: w, own: Vec::new() };
    let said = refused(&i);
    assert!(said.contains("common") && said.contains("length before") && said.contains("-1"), "{said}");

    let mut w = widths();
    w.after = f64::NAN;
    i.crosses = Crosses::Count {
        count: 3,
        common: widths(),
        own: vec![CrossOwn { index: 2, widths: w }],
    };
    let said = refused(&i);
    assert!(said.contains("cross section 2") && said.contains("length after"), "{said}");

    // A cross of no length, or with no corridor, has nothing to compute.
    let w = CrossWidths { before: 0.0, after: 0.0, left: 5.0, right: 5.0 };
    i.crosses = Crosses::Every { spacing: 25.0, common: w, own: Vec::new() };
    assert!(refused(&i).contains("no length"));
    let w = CrossWidths { before: 5.0, after: 5.0, left: 0.0, right: 0.0 };
    i.crosses = Crosses::Every { spacing: 25.0, common: w, own: Vec::new() };
    assert!(refused(&i).contains("keep nothing"));
}

#[test]
fn a_cross_set_apart_twice_is_refused() {
    let mut i = base();
    i.crosses = Crosses::Every {
        spacing: 25.0,
        common: widths(),
        own: vec![
            CrossOwn { index: 1, widths: widths() },
            CrossOwn { index: 1, widths: widths() },
        ],
    };
    assert!(refused(&i).contains("cross section 1 is set apart twice"));
}
