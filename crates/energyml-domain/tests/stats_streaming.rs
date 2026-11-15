//! The one-pass accumulator, the quantiles and their sketch, the scale of a
//! value and the histogram over it.
//!
//! Every expectation is a definition or a published formula, cited where it is
//! used; none is read from a file. Where an accumulator could agree with itself
//! and be wrong, the test computes the same thing again, the naive way, here.

use energyml_domain::stats::Accumulator;
use energyml_domain::stats::accumulate::MergeRefusal;
use energyml_domain::stats::binned::{BinRefusal, BinnedFrom, binned_exact, binned_from_sketch};
use energyml_domain::stats::quantile::{QuantileMethod, QuantileMethodUsed, QuantileRefusal, quantiles};
use energyml_domain::stats::scale::{
    AUTO_HIGH, AUTO_LOW, Fraction, LOG_DECADES, Scale, ScaleKind, ScaleRefusal, auto_bounds, propose_log,
};
use energyml_domain::stats::sketch::{LogSketch, SketchRefusal};

fn close(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} is not within {tolerance} of {b}");
}

// ---------------------------------------------------------------------------
// Accumulator
// ---------------------------------------------------------------------------

/// 1000 deterministic values, then 13 `NaN` and 2 infinities scattered in.
fn stream() -> Vec<f64> {
    let mut values: Vec<f64> = (0..1000u64)
        .map(|i| ((i * 37) % 101) as f64 * 0.5 - 7.0)
        .collect();
    for k in 0..13 {
        values.insert(k * 70, f64::NAN);
    }
    values.insert(333, f64::INFINITY);
    values.insert(901, f64::NEG_INFINITY);
    values
}

#[test]
fn merged_accumulators_equal_one_pass() {
    let values = stream();
    assert_eq!(values.len(), 1015);

    let mut whole = Accumulator::new();
    whole.extend(&values);

    let mut merged = Accumulator::new();
    for part in [&values[..333], &values[333..700], &values[700..]] {
        let mut slice = Accumulator::new();
        slice.extend(part);
        merged.merge(&slice).expect("same null value");
    }

    // The naive two-pass computation, written here: Summary is no longer
    // independent of the accumulator, it is built on it.
    let finite: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    let mean = finite.iter().sum::<f64>() / finite.len() as f64;
    let deviation =
        (finite.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / finite.len() as f64).sqrt();

    for accumulator in [&whole, &merged] {
        assert_eq!(accumulator.count(), 1015);
        assert_eq!(accumulator.finite(), 1000);
        assert_eq!(accumulator.not_a_number(), 13);
        assert_eq!(accumulator.infinite(), 2);
        assert_eq!(accumulator.non_finite(), 15);
        let moments = accumulator.moments().expect("finite values");
        assert_eq!(moments.min, finite.iter().copied().fold(f64::INFINITY, f64::min));
        assert_eq!(
            moments.max,
            finite.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        );
        close(moments.mean, mean, 1e-12);
        close(moments.deviation, deviation, 1e-12);
    }
    // The merge of the logarithmic moments too (Chan, Golub and LeVeque 1983).
    let (a, b) = (
        whole.positive_moments().unwrap(),
        merged.positive_moments().unwrap(),
    );
    assert_eq!(a.count, b.count);
    close(a.mean, b.mean, 1e-12);
    close(a.deviation, b.deviation, 1e-12);
    assert_eq!(whole.non_positive(), merged.non_positive());
}

#[test]
fn a_null_value_is_counted_apart_and_never_averaged() {
    // RESQML IntegerExternalArray.NullValue: "no data here", not a number.
    let mut accumulator = Accumulator::with_null_value(Some(-999));
    for value in [4, -999, 8, 6] {
        accumulator.push_integer(value);
    }
    assert_eq!(accumulator.null(), 1);
    assert_eq!(accumulator.finite(), 3);
    assert_eq!(accumulator.count(), 4);
    assert_eq!(accumulator.moments().unwrap().mean, 6.0);
}

#[test]
fn decades_ignore_non_positive_values_and_count_them() {
    // A logarithm is not defined at zero: the zero and the negative are counted.
    let mut accumulator = Accumulator::new();
    accumulator.extend(&[0.0, -1.0, 0.11, 6393.0]);
    assert_eq!(accumulator.non_positive(), 2);
    close(accumulator.decades().unwrap(), (6393.0f64 / 0.11).log10(), 1e-12);
    // One positive value spans no decade, which is not the same as no value.
    let mut one = Accumulator::new();
    one.push(5.0);
    assert_eq!(one.decades(), Some(0.0));
}

#[test]
fn an_empty_stream_has_no_moments() {
    let empty = Accumulator::new();
    assert_eq!(empty.moments(), None);
    assert_eq!(empty.decades(), None);
    assert_eq!(empty.positive_moments(), None);

    let mut holes = Accumulator::new();
    holes.extend(&[f64::NAN, f64::NAN]);
    assert_eq!(holes.moments(), None);
    assert_eq!(holes.decades(), None);
    assert_eq!(holes.count(), 2);
}

#[test]
fn merging_two_null_values_is_refused() {
    let mut ours = Accumulator::with_null_value(Some(-1));
    let theirs = Accumulator::with_null_value(Some(-999));
    let refused = ours.merge(&theirs);
    assert_eq!(
        refused,
        Err(MergeRefusal::DifferentNullValue {
            ours: Some(-1),
            theirs: Some(-999)
        })
    );
}

// ---------------------------------------------------------------------------
// Quantiles and the sketch
// ---------------------------------------------------------------------------

#[test]
fn exact_quantiles_follow_hyndman_and_fan_type_7() {
    // Hyndman and Fan 1996, type 7: h = (n - 1) q over the sorted values.
    let q = quantiles(
        &[1.0, 2.0, 3.0, 4.0],
        &[0.5, 0.25, 0.0, 1.0],
        QuantileMethod::Exact,
    )
    .unwrap();
    assert_eq!(q.values, vec![2.5, 1.75, 1.0, 4.0]);
    assert_eq!(q.method, QuantileMethodUsed::Exact);
    assert_eq!(q.at, vec![0.5, 0.25, 0.0, 1.0]);
}

#[test]
fn quantiles_set_non_finite_values_aside() {
    let q = quantiles(
        &[f64::NAN, 1.0, 2.0, f64::INFINITY, 3.0],
        &[0.5],
        QuantileMethod::Auto,
    )
    .unwrap();
    assert_eq!(q.values, vec![2.0]);
    assert_eq!(q.finite, 3);
    assert_eq!(q.set_aside, 2);
}

#[test]
fn selection_and_sorting_agree() {
    let values: Vec<f64> = (0..1000u64).map(|i| ((i * 7919) % 1009) as f64 * 0.25).collect();
    let few = [0.1, 0.5, 0.9];
    let mut many = vec![0.1, 0.5, 0.9];
    many.extend((0..17).map(|k| k as f64 / 16.0));
    assert_eq!(many.len(), 20);
    let by_selection = quantiles(&values, &few, QuantileMethod::Exact).unwrap();
    let by_sorting = quantiles(&values, &many, QuantileMethod::Exact).unwrap();
    assert_eq!(by_selection.values, by_sorting.values[..3].to_vec());
}

#[test]
fn sketch_quantiles_are_within_alpha_of_exact() {
    // DDSketch (Masson, Rim and Lee, PVLDB 12(12), 2019): a quantile is within
    // the relative error of the order statistic it stands for.
    // TODO(verify): the number of the theorem in the article.
    let values: Vec<f64> = (0..100_000u64)
        .map(|i| 10f64.powf((i % 1000) as f64 / 200.0))
        .collect();
    let mut sorted = values.clone();
    sorted.sort_by(f64::total_cmp);
    let at = [0.01, 0.1, 0.5, 0.9, 0.99];
    let found = quantiles(&values, &at, QuantileMethod::Sketch { relative_error: 0.01 }).unwrap();
    assert_eq!(found.method, QuantileMethodUsed::Sketch { relative_error: 0.01 });
    for (q, s) in at.iter().zip(&found.values) {
        let rank = (q * (sorted.len() - 1) as f64).floor() as usize;
        let x = sorted[rank];
        assert!(
            (s - x).abs() <= 0.01 * x.abs() * 1.000_001,
            "q {q}: sketch {s}, order statistic {x}"
        );
    }
}

#[test]
fn sketches_merge_into_the_sketch_of_the_whole() {
    let values: Vec<f64> = (0..2000u64).map(|i| ((i * 37) % 401) as f64 - 200.0).collect();
    let mut whole = LogSketch::new(0.01).unwrap();
    let (mut first, mut second) = (LogSketch::new(0.01).unwrap(), LogSketch::new(0.01).unwrap());
    for (i, v) in values.iter().enumerate() {
        whole.push(*v);
        if i < 1000 { first.push(*v) } else { second.push(*v) }
    }
    first.merge(&second).unwrap();
    assert_eq!(first.buckets(), whole.buckets());
    assert_eq!(first.count(), whole.count());
    assert_eq!(first.quantile(0.5), whole.quantile(0.5));
}

#[test]
fn a_sketch_holds_negative_values_and_zero() {
    let mut sketch = LogSketch::new(0.01).unwrap();
    for v in [-100.0, -1.0, 0.0, 1.0, 100.0] {
        sketch.push(v);
    }
    assert_eq!(sketch.quantile(0.5), Some(0.0));
    let low = sketch.quantile(0.25).unwrap();
    assert!((low + 1.0).abs() <= 0.01 * 1.000_001, "{low}");
    let high = sketch.quantile(0.75).unwrap();
    assert!((high - 1.0).abs() <= 0.01 * 1.000_001, "{high}");
    assert_eq!(sketch.quantile(0.0), Some(-100.0));
    assert_eq!(sketch.quantile(1.0), Some(100.0));
    assert_eq!(sketch.positive_count(), 2);
    // Over the positive values only, for a logarithmic scale.
    let positive = sketch.positive_quantile(0.0).unwrap();
    assert_eq!(positive, 1.0);
}

#[test]
fn sketches_of_two_accuracies_do_not_merge() {
    let mut a = LogSketch::new(0.01).unwrap();
    let b = LogSketch::new(0.02).unwrap();
    assert_eq!(
        a.merge(&b),
        Err(SketchRefusal::DifferentRelativeError {
            ours: 0.01,
            theirs: 0.02
        })
    );
    assert!(matches!(
        LogSketch::new(0.0),
        Err(SketchRefusal::RelativeErrorOutOfRange { .. })
    ));
    assert!(matches!(
        LogSketch::new(0.6),
        Err(SketchRefusal::RelativeErrorOutOfRange { .. })
    ));
}

#[test]
fn a_quantile_outside_zero_one_is_refused() {
    assert_eq!(
        quantiles(&[1.0, 2.0], &[1.5], QuantileMethod::Auto),
        Err(QuantileRefusal::OutOfRange { at: 1.5 })
    );
    assert_eq!(
        quantiles(&[1.0, 2.0], &[], QuantileMethod::Auto),
        Err(QuantileRefusal::NothingAsked)
    );
    assert_eq!(
        quantiles(&[f64::NAN], &[0.5], QuantileMethod::Auto),
        Err(QuantileRefusal::NoFiniteValue { set_aside: 1 })
    );
}

// ---------------------------------------------------------------------------
// Scale
// ---------------------------------------------------------------------------

#[test]
fn a_log_scale_refuses_non_positive_bounds() {
    let refused = Scale::new(ScaleKind::Log10, 0.0, 10.0, false);
    assert!(
        matches!(refused, Err(ScaleRefusal::LogBoundNotPositive { .. })),
        "{refused:?}"
    );
    assert!(matches!(
        Scale::new(ScaleKind::Linear, 5.0, 1.0, false),
        Err(ScaleRefusal::Reversed { .. })
    ));
    assert!(matches!(
        Scale::new(ScaleKind::Linear, 5.0, 5.0, false),
        Err(ScaleRefusal::Degenerate { .. })
    ));
    assert!(matches!(
        Scale::new(ScaleKind::Linear, f64::NAN, 5.0, false),
        Err(ScaleRefusal::NotFinite { .. })
    ));
}

#[test]
fn a_value_at_or_below_zero_is_not_positive_not_coloured() {
    let scale = Scale::new(ScaleKind::Log10, 0.1, 1000.0, false).unwrap();
    assert_eq!(scale.fraction(0.0), Fraction::NotPositive);
    assert_eq!(scale.fraction(-3.0), Fraction::NotPositive);
    assert_eq!(scale.fraction(f64::NAN), Fraction::Gap);
}

#[test]
fn a_decade_is_a_third_of_a_three_decade_scale() {
    let log = Scale::new(ScaleKind::Log10, 1.0, 1000.0, false).unwrap();
    let Fraction::At(f) = log.fraction(10.0) else {
        panic!("10 is inside 1..1000")
    };
    close(f, 1.0 / 3.0, 1e-12);
    let linear = Scale::new(ScaleKind::Linear, 1.0, 1000.0, false).unwrap();
    let Fraction::At(f) = linear.fraction(10.0) else {
        panic!("10 is inside 1..1000")
    };
    close(f, 9.0 / 999.0, 1e-12);
    close(log.decades().unwrap(), 3.0, 1e-12);
}

#[test]
fn reverse_mirrors_the_fraction_not_the_bounds() {
    let scale = Scale::new(ScaleKind::Linear, 0.0, 10.0, true).unwrap();
    assert_eq!(scale.min(), 0.0);
    assert_eq!(scale.max(), 10.0);
    assert_eq!(scale.fraction(0.0), Fraction::At(1.0));
    assert_eq!(scale.fraction(10.0), Fraction::At(0.0));
    // Above and Below speak of the value, not of the direction.
    assert_eq!(scale.fraction(11.0), Fraction::Above);
    assert_eq!(scale.fraction(-1.0), Fraction::Below);
}

#[test]
fn value_at_inverts_fraction() {
    for reverse in [false, true] {
        let scale = Scale::new(ScaleKind::Log10, 0.11, 6393.0, reverse).unwrap();
        for value in [0.11, 0.5, 7.0, 123.0, 6393.0] {
            let Fraction::At(f) = scale.fraction(value) else {
                panic!("{value} is inside the scale")
            };
            let back = scale.value_at(f);
            assert!(
                ((back - value) / value).abs() < 1e-9,
                "{value} came back as {back}"
            );
        }
    }
}

#[test]
fn log_is_proposed_from_three_decades() {
    assert!(!propose_log(Some(2.99)));
    assert!(propose_log(Some(LOG_DECADES)));
    assert!(!propose_log(None));
}

#[test]
fn automatic_bounds_are_the_first_and_ninety_ninth_percentiles() {
    assert_eq!((AUTO_LOW, AUTO_HIGH), (0.01, 0.99));
    let linear: Vec<f64> = (0..=100).map(f64::from).collect();
    let (lo, hi) = auto_bounds(&linear, ScaleKind::Linear, QuantileMethod::Exact).unwrap();
    close(lo, 1.0, 1e-9);
    close(hi, 99.0, 1e-9);
    // On a logarithmic scale the values at or below zero are set aside: kept,
    // they would change the population and with it the bounds.
    let mut log: Vec<f64> = (1..=101).map(f64::from).collect();
    log.extend([-5.0, 0.0]);
    let (lo, hi) = auto_bounds(&log, ScaleKind::Log10, QuantileMethod::Exact).unwrap();
    close(lo, 2.0, 1e-9);
    close(hi, 100.0, 1e-9);
    assert!(matches!(
        auto_bounds(&[-1.0, 0.0], ScaleKind::Log10, QuantileMethod::Exact),
        Err(ScaleRefusal::NoValues { .. })
    ));
}

#[test]
fn transform_refuses_a_non_positive_value_by_name() {
    let scale = Scale::new(ScaleKind::Log10, 1.0, 10.0, false).unwrap();
    assert_eq!(
        scale.transform(-1.0),
        Err(ScaleRefusal::NotPositive { value: -1.0 })
    );
    assert_eq!(scale.transform(100.0), Ok(2.0));
}

// ---------------------------------------------------------------------------
// Binned
// ---------------------------------------------------------------------------

#[test]
fn the_maximum_lands_in_the_last_bin() {
    // The same split as the Summary histogram: the last bin is closed.
    let values: Vec<f64> = (0..=9).map(f64::from).collect();
    let scale = Scale::new(ScaleKind::Linear, 0.0, 9.0, false).unwrap();
    let binned = binned_exact(&values, &scale, 4).unwrap();
    assert_eq!(binned.counts, vec![3, 2, 2, 3]);
    assert_eq!(binned.total(), 10);
    assert_eq!(binned.bins(), 4);
    assert_eq!(binned.from, BinnedFrom::Values);
}

#[test]
fn log_bins_are_equal_decades() {
    let scale = Scale::new(ScaleKind::Log10, 1.0, 1000.0, false).unwrap();
    let binned = binned_exact(&[1.0, 5.0, 10.0, 50.0, 100.0, 500.0], &scale, 3).unwrap();
    assert_eq!(binned.counts, vec![2, 2, 2]);
    let (lo, hi) = binned.edges(1);
    close(lo, 10.0, 1e-9);
    close(hi, 100.0, 1e-9);
    // The middle of the second decade, in the space of the scale.
    close(binned.center(1), 10f64.powf(1.5), 1e-9);
}

#[test]
fn values_outside_the_scale_are_counted_apart() {
    let scale = Scale::new(ScaleKind::Log10, 0.1, 10.0, false).unwrap();
    let binned = binned_exact(&[-1.0, 0.0, 0.01, 1.0, 100.0, f64::NAN], &scale, 4).unwrap();
    assert_eq!(binned.not_positive, 2);
    assert_eq!(binned.below, 1);
    assert_eq!(binned.above, 1);
    assert_eq!(binned.not_finite, 1);
    assert_eq!(binned.total(), 1);
}

#[test]
fn bins_from_a_sketch_say_they_are_approximate() {
    let mut sketch = LogSketch::new(0.01).unwrap();
    for v in [1.0, 10.0, 100.0] {
        sketch.push(v);
    }
    let scale = Scale::new(ScaleKind::Log10, 0.1, 1000.0, false).unwrap();
    let binned = binned_from_sketch(&sketch, &scale, 8).unwrap();
    assert_eq!(binned.from, BinnedFrom::Sketch { relative_error: 0.01 });
    assert_eq!(binned.total(), 3);
}

#[test]
fn a_histogram_of_no_bin_or_too_many_is_refused() {
    let scale = Scale::new(ScaleKind::Linear, 0.0, 1.0, false).unwrap();
    assert_eq!(binned_exact(&[0.5], &scale, 0), Err(BinRefusal::NoBin));
    assert!(matches!(
        binned_exact(&[0.5], &scale, 100_000),
        Err(BinRefusal::TooManyBins { bins: 100_000, .. })
    ));
}
