//! Settled steps, the injectivity index and the Hall plot
//!
//!
//! Every expected value here is computed by hand in the comment beside it -
//! never by calling the module under test a second way. The recovery of an
//! index from a synthesized test lives in
//! `crates/energyml-build/tests/injectivity.rs`, against the parameter that
//! synthesized it.

use energyml_analysis::hall::{HallError, hall};
use energyml_analysis::step_rate::{Series, SeriesError, Settle, StepRateError, injectivity, steps};

const P0: f64 = 100.0;

/// Hourly samples: each step `(rate, pressures)` in turn.
fn series_of(steps: &[(f64, &[f64])]) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let (mut hours, mut rates, mut pressures) = (Vec::new(), Vec::new(), Vec::new());
    for (rate, values) in steps {
        for value in *values {
            hours.push(hours.len() as f64);
            rates.push(*rate);
            pressures.push(*value);
        }
    }
    (hours, rates, pressures)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

const STILL: Settle = Settle {
    window: 3,
    tolerance: 0.01,
};

#[test]
fn the_fit_of_three_settled_steps_is_the_one_computed_by_hand() {
    // Points (q, Δp): (10, 1), (20, 3), (30, 4). q̄ = 20, Δp̄ = 8/3;
    // Sxx = 200, Sxy = (−10)(−5/3) + 0 + 10(4/3) = 30 → slope 0.15,
    // intercept 8/3 − 3 = −1/3; residuals −1/6, 1/3, −1/6;
    // SS_res = 1/6, SS_tot = 42/9 → R² = 1 − (1/6)/(14/3) = 27/28;
    // index 1 / 0.15 = 20/3.
    // To break: regress q against Δp, or take the mean of the window
    // where the pressure has not moved anyway - the first turns red here.
    let (h, q, p) = series_of(&[
        (10.0, &[P0 + 1.0; 4]),
        (20.0, &[P0 + 3.0; 4]),
        (30.0, &[P0 + 4.0; 4]),
    ]);
    let series = Series::new(&h, &q, &p).unwrap();
    let found = injectivity(&series, &STILL, P0).unwrap();
    assert!(close(found.fit.slope, 0.15), "{:?}", found.fit);
    assert!(close(found.fit.intercept, -1.0 / 3.0), "{:?}", found.fit);
    assert!(close(found.index, 20.0 / 3.0), "{}", found.index);
    let expected = [-1.0 / 6.0, 1.0 / 3.0, -1.0 / 6.0];
    assert_eq!(found.fit.residuals.len(), 3);
    assert!(
        found
            .fit
            .residuals
            .iter()
            .zip(expected)
            .all(|(a, b)| close(*a, b)),
        "{:?}",
        found.fit.residuals
    );
    assert!(close(found.fit.r_squared, 27.0 / 28.0), "{}", found.fit.r_squared);
    assert_eq!(
        found.points.iter().map(|one| one.rank).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(found.set_aside.is_empty());
    // q / Δp step by step: 10, 20/3, 7.5 - reported beside, never the index.
    let ratios: Vec<f64> = found
        .ratios
        .iter()
        .map(|one| one.as_ref().unwrap().index)
        .collect();
    assert!(
        ratios
            .iter()
            .zip([10.0, 20.0 / 3.0, 7.5])
            .all(|(a, b)| close(*a, b)),
        "{ratios:?}"
    );
}

#[test]
fn a_step_that_never_settles_is_refused_by_its_rank_and_not_read_at_its_last_sample() {
    // Step 2 climbs 1 bar an hour to its end. Read at its last sample it
    // would give Δp = 5 at q = 20 and bend the line; it must be set aside.
    // The two others give (10, 1), (30, 3): slope 0.1, index 10.
    // To break: take an unsettled step at its last sample (the index moves)
    // or drop it without saying (`set_aside` empties).
    let (h, q, p) = series_of(&[
        (10.0, &[P0 + 1.0; 4]),
        (20.0, &[P0 + 2.0, P0 + 3.0, P0 + 4.0, P0 + 5.0]),
        (30.0, &[P0 + 3.0; 4]),
    ]);
    let series = Series::new(&h, &q, &p).unwrap();
    let all = steps(&series, &STILL).unwrap();
    assert_eq!(
        all.iter()
            .map(|one| (one.rank, one.rate, one.first, one.samples))
            .collect::<Vec<_>>(),
        [(1, 10.0, 0, 4), (2, 20.0, 4, 4), (3, 30.0, 8, 4)]
    );
    match &all[1].verdict {
        Err(StepRateError::NotSettled {
            rank: 2,
            steepest,
            window: 3,
            ..
        }) => assert!(close(*steepest, 1.0), "{steepest}"),
        other => panic!("step 2 refused by its rank, not {other:?}"),
    }
    let found = injectivity(&series, &STILL, P0).unwrap();
    assert_eq!(
        found.points.iter().map(|one| one.rank).collect::<Vec<_>>(),
        [1, 3]
    );
    assert!(
        matches!(
            found.set_aside.as_slice(),
            [StepRateError::NotSettled { rank: 2, .. }]
        ),
        "{:?}",
        found.set_aside
    );
    assert!(close(found.index, 10.0), "{}", found.index);
}

#[test]
fn fewer_than_two_settled_steps_make_no_line() {
    let moving: &[f64] = &[P0 + 1.0, P0 + 2.0, P0 + 3.0, P0 + 4.0];
    let (h, q, p) = series_of(&[(10.0, moving), (20.0, moving)]);
    let series = Series::new(&h, &q, &p).unwrap();
    assert_eq!(
        injectivity(&series, &STILL, P0),
        Err(StepRateError::TooFewSettled {
            settled: 0,
            set_aside: vec![1, 2]
        })
    );
    let (h, q, p) = series_of(&[(10.0, &[P0 + 1.0; 4]), (20.0, moving)]);
    let series = Series::new(&h, &q, &p).unwrap();
    assert_eq!(
        injectivity(&series, &STILL, P0),
        Err(StepRateError::TooFewSettled {
            settled: 1,
            set_aside: vec![2]
        })
    );
}

#[test]
fn the_window_is_the_last_k_samples_of_the_step_and_no_more() {
    // The pressure moves between samples 1 and 2 of the step, then stays:
    // the last 3 samples are still (2 differences of 0), the last 4 are not.
    // To break: count k differences instead of k − 1, or look at the first
    // samples of the step - one of the two assertions turns red.
    let values: &[f64] = &[P0, P0 + 1.0, P0 + 1.0, P0 + 1.0];
    let (h, q, p) = series_of(&[(10.0, values)]);
    let series = Series::new(&h, &q, &p).unwrap();
    let three = steps(
        &series,
        &Settle {
            window: 3,
            tolerance: 0.01,
        },
    )
    .unwrap();
    assert_eq!(three[0].verdict.as_ref().map(|one| one.pressure), Ok(P0 + 1.0));
    let four = steps(
        &series,
        &Settle {
            window: 4,
            tolerance: 0.01,
        },
    )
    .unwrap();
    assert!(
        matches!(four[0].verdict, Err(StepRateError::NotSettled { rank: 1, .. })),
        "{:?}",
        four[0].verdict
    );
    let five = steps(
        &series,
        &Settle {
            window: 5,
            tolerance: 0.01,
        },
    )
    .unwrap();
    assert_eq!(
        five[0].verdict,
        Err(StepRateError::TooShort {
            rank: 1,
            samples: 4,
            window: 5
        })
    );
}

#[test]
fn a_hole_inside_a_window_refuses_the_step_and_one_outside_changes_nothing() {
    let (h, q, p) = series_of(&[
        (10.0, &[f64::NAN, P0 + 1.0, P0 + 1.0, P0 + 1.0]),
        (20.0, &[P0 + 2.0, P0 + 2.0, f64::NAN, P0 + 2.0]),
    ]);
    let series = Series::new(&h, &q, &p).unwrap();
    let all = steps(&series, &STILL).unwrap();
    assert_eq!(
        all[0].verdict.as_ref().map(|one| one.pressure),
        Ok(P0 + 1.0),
        "the hole is before the window"
    );
    // Sample 7 of the series is the third of step 2, inside its window.
    assert_eq!(
        all[1].verdict,
        Err(StepRateError::NanInWindow { rank: 2, sample: 7 })
    );
}

#[test]
fn a_zero_rate_enters_the_line_but_no_ratio() {
    // (0, 0), (10, 1), (20, 2): slope 0.1, index 10. Step 1 has no q / Δp.
    let (h, q, p) = series_of(&[(0.0, &[P0; 3]), (10.0, &[P0 + 1.0; 3]), (20.0, &[P0 + 2.0; 3])]);
    let series = Series::new(&h, &q, &p).unwrap();
    let found = injectivity(&series, &STILL, P0).unwrap();
    assert!(close(found.index, 10.0));
    assert_eq!(
        found.ratios[0],
        Err(StepRateError::ZeroRate {
            rank: 1,
            delta_p: 0.0
        })
    );
    assert!(close(found.ratios[1].as_ref().unwrap().index, 10.0));
    // A rate that settles at the static pressure: no ratio either.
    let (h, q, p) = series_of(&[(10.0, &[P0; 3]), (20.0, &[P0 + 1.0; 3])]);
    let series = Series::new(&h, &q, &p).unwrap();
    let found = injectivity(&series, &STILL, P0).unwrap();
    assert_eq!(
        found.ratios[0],
        Err(StepRateError::NoPressureRise { rank: 1, rate: 10.0 })
    );
}

#[test]
fn a_line_without_a_rise_or_without_two_rates_is_refused() {
    // Steps 1 and 3 at 10, step 2 unsettled: two settled points, one rate.
    let (h, q, p) = series_of(&[
        (10.0, &[P0 + 1.0; 3]),
        (20.0, &[P0 + 1.0, P0 + 2.0, P0 + 3.0]),
        (10.0, &[P0 + 1.0; 3]),
    ]);
    let series = Series::new(&h, &q, &p).unwrap();
    assert_eq!(
        injectivity(&series, &STILL, P0),
        Err(StepRateError::OneRate { rate: 10.0 })
    );
    // The pressure falls as the rate rises.
    let (h, q, p) = series_of(&[(10.0, &[P0 + 2.0; 3]), (20.0, &[P0 + 1.0; 3])]);
    let series = Series::new(&h, &q, &p).unwrap();
    assert!(matches!(
        injectivity(&series, &STILL, P0),
        Err(StepRateError::NoRise { .. })
    ));
}

#[test]
fn what_is_no_step_rate_test_is_refused_by_name() {
    let (h, q, p) = (vec![0.0, 1.0, 2.0], vec![1.0, 1.0, 1.0], vec![P0, P0, P0]);
    assert_eq!(
        Series::new(&h, &q, &p[..2]),
        Err(SeriesError::MismatchedLengths {
            hours: 3,
            rates: 3,
            pressures: 2
        })
    );
    assert_eq!(
        Series::new(&[], &[], &[]),
        Err(SeriesError::TooFewSamples { given: 0, needed: 1 })
    );
    assert_eq!(
        Series::new(&[0.0, 1.0, 1.0], &q, &p),
        Err(SeriesError::NotIncreasing {
            rank: 3,
            hours: 1.0,
            before: 1.0
        }),
        "two samples at one instant"
    );
    assert!(matches!(
        Series::new(&[0.0, 2.0, 1.0], &q, &p),
        Err(SeriesError::NotIncreasing { rank: 3, .. })
    ));
    assert!(matches!(
        Series::new(&[0.0, f64::NAN, 2.0], &q, &p),
        Err(SeriesError::NotFinite {
            rank: 2,
            what: "instant",
            ..
        })
    ));
    assert!(matches!(
        Series::new(&h, &[1.0, 1.0, f64::NAN], &p),
        Err(SeriesError::NotFinite {
            rank: 3,
            what: "rate",
            ..
        })
    ));
    let series = Series::new(&h, &q, &p).unwrap();
    assert_eq!(
        steps(
            &series,
            &Settle {
                window: 1,
                tolerance: 0.01
            }
        ),
        Err(StepRateError::BadWindow { given: 1 })
    );
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            matches!(
                steps(
                    &series,
                    &Settle {
                        window: 2,
                        tolerance: bad
                    }
                ),
                Err(StepRateError::BadTolerance { .. })
            ),
            "{bad}"
        );
    }
    assert!(matches!(
        injectivity(&series, &STILL, f64::NAN),
        Err(StepRateError::BadStaticPressure { .. })
    ));
}

// -- Hall --------------------------------------------------------------------

#[test]
fn a_constant_rate_against_a_constant_rise_is_a_line_of_slope_rise_over_rate() {
    // q = 30 m3/h, Δp = 3 bar at every sample: ∫q dt = 30 t m3 and
    // ∫Δp dt = 3 t bar·h, whatever the sampling - slope 3 / 30 = 0.1 bar·h/m3,
    // the inverse of an index of 10 m3/h/bar.
    // To break: integrate p instead of p − p_static (the y values move by
    // P0·t), or drop the dt (the uneven sampling shows).
    let h = [0.0, 0.5, 2.0, 3.5, 10.0];
    let q = [30.0; 5];
    let p = [P0 + 3.0; 5];
    let plot = hall(&Series::new(&h, &q, &p).unwrap(), P0).unwrap();
    for (at, t) in h.iter().enumerate() {
        assert!(
            close(plot.volume[at], 30.0 * t),
            "volume at {t} h: {}",
            plot.volume[at]
        );
        assert!(
            close(plot.pressure_time[at], 3.0 * t),
            "∫Δp dt at {t} h: {}",
            plot.pressure_time[at]
        );
    }
    let slope = (plot.pressure_time[4] - plot.pressure_time[1]) / (plot.volume[4] - plot.volume[1]);
    assert!(close(slope, 0.1), "{slope}");
}

#[test]
fn the_integral_is_a_trapezoid() {
    // Two samples 2 h apart, 10 then 20 m3/h: (10 + 20) / 2 × 2 = 30 m3 -
    // a left rectangle would say 20, a right one 40. The pressure rises from
    // 0 to 4 bar above static: (0 + 4) / 2 × 2 = 4 bar·h.
    let plot = hall(
        &Series::new(&[0.0, 2.0], &[10.0, 20.0], &[P0, P0 + 4.0]).unwrap(),
        P0,
    )
    .unwrap();
    assert_eq!(plot.volume, [0.0, 30.0]);
    assert_eq!(plot.pressure_time, [0.0, 4.0]);
}

#[test]
fn a_hall_plot_refuses_a_hole_and_a_single_sample() {
    let h = [0.0, 1.0, 2.0];
    let q = [10.0; 3];
    assert!(matches!(
        hall(&Series::new(&h, &q, &[P0, f64::NAN, P0]).unwrap(), P0),
        Err(HallError::NoPressure { rank: 2, .. })
    ));
    assert_eq!(
        hall(&Series::new(&[0.0], &[10.0], &[P0]).unwrap(), P0),
        Err(HallError::Series(SeriesError::TooFewSamples {
            given: 1,
            needed: 2
        }))
    );
    assert!(matches!(
        hall(&Series::new(&h, &q, &[P0; 3]).unwrap(), f64::INFINITY),
        Err(HallError::BadStaticPressure { .. })
    ));
}
