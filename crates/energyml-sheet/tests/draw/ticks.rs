//! Graduations: one rule for the section (`sec/src/layout/ticks.rs`), the
//! studio (`tracks.ts` `niceTicks` and `logTicks`) and the desktop
//! (`scale_bar.rs`). Each test says what would have to break for it to fail.

use energyml_sheet::draw::ticks::{
    LogDensity, TickRefusal, label, log_label, log_ticks, nice_step_at_least, nice_step_at_most, nice_ticks,
};

#[test]
fn nice_ticks_step_is_one_two_or_five_times_a_power_of_ten() {
    // Breaks if `tidy` is removed: 3 x 0.1 is 0.30000000000000004, not 0.3.
    let t = nice_ticks(0.0, 1000.0, 5.0).unwrap();
    assert_eq!(t.step, 200.0);
    assert_eq!(t.values, vec![0.0, 200.0, 400.0, 600.0, 800.0, 1000.0]);
    let t = nice_ticks(123.0, 187.0, 5.0).unwrap();
    assert_eq!(t.step, 20.0);
    assert_eq!(t.values, vec![140.0, 160.0, 180.0]);
    let t = nice_ticks(0.0, 0.37, 5.0).unwrap();
    assert_eq!(t.step, 0.1);
    assert_eq!(t.values, vec![0.0, 0.1, 0.2, 0.3]);
}

#[test]
fn nice_ticks_are_ascending_whatever_the_order_of_the_bounds() {
    // Breaks if the bounds are not put in order before the multiples are taken.
    assert_eq!(nice_ticks(1000.0, 0.0, 5.0), nice_ticks(0.0, 1000.0, 5.0));
}

#[test]
fn nice_ticks_never_exceed_the_target_plus_one() {
    // Breaks if the step is the nearest round number instead of the smallest
    // one that is at least `range / target`: 0..700 would get a step of 100 and
    // 8 values.
    for (a, b, target) in [
        (0.0, 700.0, 5.0),
        (0.0, 1.0, 3.0),
        (-5300.0, -4100.0, 7.3),
        (0.0, 0.37, 2.5),
    ] {
        let t = nice_ticks(a, b, target).unwrap();
        assert!(
            t.values.len() <= target.floor() as usize + 1,
            "{} values for a target of {target} over {a}..{b}",
            t.values.len()
        );
    }
    assert_eq!(nice_ticks(0.0, 700.0, 5.0).unwrap().step, 200.0);
}

#[test]
fn nice_ticks_refuse_by_name() {
    // Breaks if equal bounds give an empty list instead of a refusal.
    assert_eq!(
        nice_ticks(5.0, 5.0, 5.0),
        Err(TickRefusal::Degenerate { at: 5.0 })
    );
    assert!(matches!(
        nice_ticks(0.0, f64::NAN, 5.0),
        Err(TickRefusal::NotFinite { .. })
    ));
    assert!(matches!(
        nice_ticks(0.0, f64::INFINITY, 5.0),
        Err(TickRefusal::NotFinite { .. })
    ));
    assert_eq!(
        nice_ticks(0.0, 1.0, 0.5),
        Err(TickRefusal::BadTarget { target: 0.5 })
    );
}

#[test]
fn a_zero_tick_is_never_negative_zero() {
    // Breaks if the "-0" treatment of the label or of `tidy` is removed.
    let t = nice_ticks(-1.0, 1.0, 4.0).unwrap();
    let zero = t.values.iter().find(|v| **v == 0.0).expect("a zero tick");
    assert!(zero.is_sign_positive());
    assert_eq!(label(-0.0, 0.5), "0.0");
}

#[test]
fn log_ticks_fine_then_decades_then_every_few() {
    // Breaks if the decades step is skipped: (1, 100, 5) would go to `Every`.
    let t = log_ticks(1.0, 10.0, 5.0).unwrap();
    assert_eq!(t.density, LogDensity::Fine);
    assert_eq!(t.values, vec![1.0, 2.0, 5.0, 10.0]);
    let t = log_ticks(1.0, 100.0, 5.0).unwrap();
    assert_eq!(t.density, LogDensity::Decades);
    assert_eq!(t.values, vec![1.0, 10.0, 100.0]);
    let t = log_ticks(1e-3, 1e6, 4.0).unwrap();
    assert_eq!(t.density, LogDensity::Every(3));
    assert_eq!(t.values, vec![0.001, 1.0, 1000.0, 1e6]);
}

#[test]
fn log_ticks_refuse_non_positive_bounds() {
    // Breaks if the positivity check is removed (log10 of 0 is -inf).
    assert!(matches!(
        log_ticks(0.0, 10.0, 5.0),
        Err(TickRefusal::NotPositive { .. })
    ));
    assert!(matches!(
        log_ticks(-1.0, 10.0, 5.0),
        Err(TickRefusal::NotPositive { .. })
    ));
}

#[test]
fn nice_steps_are_exact_and_bounded() {
    // Breaks if the slack is removed: 200 asked, 500 may come back.
    assert_eq!(nice_step_at_most(0.07), Some(0.05));
    assert_eq!(nice_step_at_most(1999.0), Some(1000.0));
    assert_eq!(nice_step_at_least(0.07), Some(0.1));
    assert_eq!(nice_step_at_least(201.0), Some(500.0));
    assert_eq!(nice_step_at_least(200.0), Some(200.0));
    assert_eq!(nice_step_at_most(0.0), None);
    assert_eq!(nice_step_at_least(f64::NAN), None);
}

#[test]
fn a_label_has_the_decimals_its_step_needs() {
    // Breaks if the decimals use `ceil` instead of `floor` of the log.
    assert_eq!(label(0.30000000000000004, 0.1), "0.3");
    assert_eq!(label(0.05, 0.05), "0.05");
    assert_eq!(label(1200.0, 200.0), "1200");
    assert_eq!(log_label(0.02), "0.02");
    assert_eq!(log_label(5000.0), "5000");
    assert_eq!(log_label(1e6), "1e6");
}
