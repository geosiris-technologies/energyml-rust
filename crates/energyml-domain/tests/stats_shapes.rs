//! The shape of a distribution (modes, thresholds, the number of bars), the
//! variation along the axes of an array and its seams, pairs of sister arrays,
//! and the counting of cells by a rule.
//!
//! Data is made here; each expectation cites its source (a published method,
//! or a decision of the specification) where it is used.

use energyml_domain::stats::axes::{AxesAccumulator, AxesRefusal, Jump, STRONGEST_RATIO, strongest, variation};
use energyml_domain::stats::binned::{Binned, BinnedFrom, binned_exact};
use energyml_domain::stats::cells::{CellRule, TallyRefusal, runs, tally};
use energyml_domain::stats::modes::{
    BinInput, Mode, ModeMethod, StatMethods, ThresholdMeasure, ThresholdRefusal, VALLEY_DEPTH,
};
use energyml_domain::stats::pairs::{DensityGrid, PairAccumulator, PairRefusal};
use energyml_domain::stats::scale::{Scale, ScaleKind};

fn close(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} is not within {tolerance} of {b}");
}

fn linear(min: f64, max: f64) -> Scale {
    Scale::new(ScaleKind::Linear, min, max, false).unwrap()
}

/// A histogram written out, over a linear scale `0..counts.len()`.
fn histogram(counts: &[u64]) -> Binned {
    Binned {
        scale: linear(0.0, counts.len() as f64),
        counts: counts.to_vec(),
        below: 0,
        above: 0,
        not_positive: 0,
        not_finite: 0,
        from: BinnedFrom::Values,
    }
}

// ---------------------------------------------------------------------------
// Modes, thresholds, bars
// ---------------------------------------------------------------------------

fn two_clusters_and_a_bump() -> Vec<f64> {
    let mut values: Vec<f64> = (0..1000u64).map(|i| 1.0 + 0.001 * (i % 100) as f64).collect();
    values.extend((0..1000u64).map(|i| 100.0 + 0.1 * (i % 100) as f64));
    values.extend(std::iter::repeat_n(10.0, 30));
    values
}

#[test]
fn two_separated_clusters_give_two_modes_and_a_valley() {
    let scale = Scale::new(ScaleKind::Log10, 0.5, 200.0, false).unwrap();
    let binned = binned_exact(&two_clusters_and_a_bump(), &scale, 24).unwrap();
    let methods = StatMethods::builtin();
    let modes = methods.mode_method("histogram_peaks").unwrap().modes(&binned);
    // The bump of 30 values has a prominence of 0.03 of the tallest bar: it is
    // a peak, not a mode.
    assert_eq!(modes.len(), 2, "{modes:?}");
    assert!(modes[0].at < modes[1].at);
    let valley = methods
        .threshold_method("valley")
        .unwrap()
        .threshold(&binned, &modes)
        .unwrap();
    assert!(valley.at > 2.0 && valley.at < 50.0, "{valley:?}");
    let ThresholdMeasure::ValleyDepth(depth) = valley.measure else {
        panic!("the valley method measures a depth")
    };
    assert!(depth <= VALLEY_DEPTH, "{depth}");
    assert_eq!(valley.method, "valley");
    let (a, b) = valley.between.expect("the valley lies between two modes");
    assert!(a < valley.at && valley.at < b);
}

#[test]
fn one_cluster_gives_no_threshold() {
    let values: Vec<f64> = (0..1000u64).map(|i| 100.0 + 0.1 * (i % 100) as f64).collect();
    let scale = Scale::new(ScaleKind::Log10, 0.5, 200.0, false).unwrap();
    let binned = binned_exact(&values, &scale, 24).unwrap();
    let methods = StatMethods::builtin();
    let modes = methods.mode_method("histogram_peaks").unwrap().modes(&binned);
    assert_eq!(modes.len(), 1);
    let refused = methods.threshold_method("valley").unwrap().threshold(&binned, &modes);
    assert_eq!(refused, Err(ThresholdRefusal::Unimodal { modes: 1 }));
}

#[test]
fn a_shallow_valley_is_refused() {
    // Two modes of 100 and 90 with a valley of 60: 60 / 90 is above the limit.
    let binned = histogram(&[100, 60, 90]);
    let methods = StatMethods::builtin();
    let modes = methods.mode_method("histogram_peaks").unwrap().modes(&binned);
    assert_eq!(modes.len(), 2, "{modes:?}");
    let refused = methods.threshold_method("valley").unwrap().threshold(&binned, &modes);
    match refused {
        Err(ThresholdRefusal::Shallow { depth, limit }) => {
            close(depth, 60.0 / 90.0, 1e-12);
            assert_eq!(limit, VALLEY_DEPTH);
        }
        other => panic!("expected a shallow valley, got {other:?}"),
    }
}

#[test]
fn a_plateau_is_one_peak_stood_for_by_its_middle_bar() {
    // Bars [1, 5, 5, 5, 1]: one plateau; its middle is bin 2.
    let modes = StatMethods::builtin()
        .mode_method("histogram_peaks")
        .unwrap()
        .modes(&histogram(&[1, 5, 5, 5, 1]));
    assert_eq!(modes.len(), 1, "{modes:?}");
    assert_eq!(modes[0].bin, 2);
    assert_eq!(modes[0].height, 5);
    // An empty histogram has no mode.
    assert!(
        StatMethods::builtin()
            .mode_method("histogram_peaks")
            .unwrap()
            .modes(&histogram(&[0, 0, 0]))
            .is_empty()
    );
}

#[test]
fn otsu_splits_two_flat_blocks_between_them() {
    // Otsu 1979: the cuts after bars 1, 2 and 3 separate the same two blocks;
    // on a tie the smallest cut is kept.
    let binned = histogram(&[10, 10, 0, 0, 10, 10]);
    let threshold = StatMethods::builtin()
        .threshold_method("otsu")
        .unwrap()
        .threshold(&binned, &[])
        .unwrap();
    assert_eq!(threshold.at, binned.edges(1).1);
    assert_eq!(threshold.method, "otsu");
    assert_eq!(threshold.between, None);
    let ThresholdMeasure::Separability(eta) = threshold.measure else {
        panic!("Otsu measures a separability")
    };
    assert!((0.0..=1.0).contains(&eta), "{eta}");
    assert!(eta > 0.9, "two flat blocks are very separable: {eta}");
    // One bar alone has nothing to split.
    let alone = histogram(&[0, 7, 0]);
    assert_eq!(
        StatMethods::builtin().threshold_method("otsu").unwrap().threshold(&alone, &[]),
        Err(ThresholdRefusal::Empty)
    );
}

#[test]
fn sturges_and_freedman_diaconis_follow_their_formulas() {
    let methods = StatMethods::builtin();
    // Sturges 1926: ceil(log2 n) + 1 = ceil(9.97) + 1 = 11 for n = 1000.
    let sturges = BinInput {
        requested: 24,
        count: 1000,
        iqr: None,
        span: 10.0,
    };
    assert_eq!(methods.bin_method("sturges").unwrap().bins(&sturges), Some(11));
    // Freedman and Diaconis 1981: h = 2 iqr / cbrt(n) = 0.2, span / h = 50.
    let fd = BinInput {
        requested: 24,
        count: 1000,
        iqr: Some(1.0),
        span: 10.0,
    };
    assert_eq!(methods.bin_method("freedman_diaconis").unwrap().bins(&fd), Some(50));
    // A zero spread leaves the rule undefined: the caller falls back.
    let flat = BinInput { iqr: Some(0.0), ..fd };
    assert_eq!(methods.bin_method("freedman_diaconis").unwrap().bins(&flat), None);
    // The fixed rule is the request, kept in 1..=MAX_BINS.
    assert_eq!(methods.bin_method("fixed").unwrap().bins(&sturges), Some(24));
    let none = BinInput { requested: 0, ..sturges };
    assert_eq!(methods.bin_method("fixed").unwrap().bins(&none), Some(1));
}

struct MyModes;

impl ModeMethod for MyModes {
    fn id(&self) -> &'static str {
        "histogram_peaks"
    }

    fn modes(&self, _binned: &Binned) -> Vec<Mode> {
        vec![Mode {
            at: -12345.0,
            bin: 0,
            height: 1,
            prominence: 1.0,
        }]
    }
}

#[test]
fn the_registry_tries_the_callers_methods_first() {
    // ARCHITECTURE.md, "Registries, not match": a caller's addition is tried
    // before the built-in method of the same id.
    let mut methods = StatMethods::builtin();
    methods.push_mode_method(Box::new(MyModes));
    let found = methods
        .mode_method("histogram_peaks")
        .unwrap()
        .modes(&histogram(&[1, 5, 1]));
    assert_eq!(found[0].at, -12345.0);
    assert_eq!(methods.mode_ids(), vec!["histogram_peaks", "histogram_peaks"]);
    // An empty registry holds no method at all.
    assert!(StatMethods::none().mode_method("histogram_peaks").is_none());
    assert!(StatMethods::none().bin_ids().is_empty());
}

// ---------------------------------------------------------------------------
// Axes
// ---------------------------------------------------------------------------

/// Nine steps of 1, then a last step of `last`.
fn series_with_a_last_step(last: f64) -> Vec<f64> {
    let mut values: Vec<f64> = (0..10).map(f64::from).collect();
    values.push(9.0 + last);
    values
}

#[test]
fn a_step_ten_times_the_median_is_a_jump() {
    // S7: a step over ten times the median step of its axis is a seam. Here the
    // median step is 1 and the last step is 15 (the mean, 2.4, would hide it:
    // 15 is not over 24).
    let axes = variation(&series_with_a_last_step(15.0), &[11], ScaleKind::Linear).unwrap();
    assert_eq!(axes.len(), 1);
    assert_eq!(axes[0].median_step, Some(1.0));
    assert_eq!(
        axes[0].jumps,
        vec![Jump {
            axis: 0,
            at: 10,
            step: 15.0,
            ratio_to_median: 15.0
        }]
    );
}

#[test]
fn a_step_of_nine_times_the_median_is_not_a_jump() {
    let axes = variation(&series_with_a_last_step(9.0), &[11], ScaleKind::Linear).unwrap();
    assert!(axes[0].jumps.is_empty(), "{:?}", axes[0].jumps);
}

#[test]
fn variation_only_along_axis_1_makes_it_strongest() {
    // dims = [4, 5], v = 10 * (index along the fast axis).
    let values: Vec<f64> = (0..20).map(|k| 10.0 * (k % 5) as f64).collect();
    let axes = variation(&values, &[4, 5], ScaleKind::Linear).unwrap();
    assert!(axes[0].constant);
    assert_eq!(axes[0].mean_abs_step, Some(0.0));
    assert!(!axes[1].constant);
    assert_eq!(axes[1].mean_abs_step, Some(10.0));
    assert_eq!(strongest(&axes, STRONGEST_RATIO), Some((1, f64::INFINITY)));
}

#[test]
fn steps_carry_across_slabs_pushed_in_two_calls() {
    let values: Vec<f64> = (0..18u64).map(|k| ((k * k) % 17) as f64).collect();
    let whole = variation(&values, &[6, 3], ScaleKind::Linear).unwrap();
    let mut accumulator = AxesAccumulator::new(&[6, 3], ScaleKind::Linear).unwrap();
    accumulator.push_slabs(0, &values[..9]).unwrap();
    accumulator.push_slabs(3, &values[9..]).unwrap();
    assert_eq!(accumulator.finish(), whole);
    // And the steps along the slowest axis are really there: 5 positions of 3.
    assert_eq!(whole[0].steps, 15);
}

#[test]
fn slabs_out_of_order_are_refused() {
    let mut accumulator = AxesAccumulator::new(&[6, 3], ScaleKind::Linear).unwrap();
    let refused = accumulator.push_slabs(2, &[0.0; 3]);
    assert_eq!(refused, Err(AxesRefusal::NotContiguous { expected: 0, first: 2 }));
    assert_eq!(
        accumulator.push_slabs(0, &[0.0; 4]),
        Err(AxesRefusal::NotWholeSlabs { values: 4, slab: 3 })
    );
    let mut short = AxesAccumulator::new(&[3, 2], ScaleKind::Linear).unwrap();
    assert_eq!(
        short.push_slabs(0, &[0.0; 8]),
        Err(AxesRefusal::PastTheEnd { slabs: 1, len: 3 })
    );
    assert!(matches!(
        AxesAccumulator::new(&[], ScaleKind::Linear),
        Err(AxesRefusal::NoDimension)
    ));
}

#[test]
fn a_hole_makes_no_step() {
    // A hole is not bridged (the same rule as series decimation): |3 - 1| is
    // not a step.
    let axes = variation(&[1.0, f64::NAN, 3.0], &[3], ScaleKind::Linear).unwrap();
    assert_eq!(axes[0].steps, 0);
    assert_eq!(axes[0].mean_abs_step, None);
    assert_eq!(axes[0].median_step, None);
    assert!(!axes[0].constant);
}

#[test]
fn log_steps_are_steps_of_decades() {
    let axes = variation(&[1.0, 10.0, 100.0], &[3], ScaleKind::Log10).unwrap();
    close(axes[0].mean_abs_step.unwrap(), 1.0, 1e-12);
    // A value that is not positive is a hole on a logarithmic scale.
    let holes = variation(&[1.0, -10.0, 100.0], &[3], ScaleKind::Log10).unwrap();
    assert_eq!(holes[0].steps, 0);
}

#[test]
fn an_axis_of_length_one_says_nothing_rather_than_zero() {
    let axes = variation(&[1.0, 2.0, 3.0], &[1, 3], ScaleKind::Linear).unwrap();
    assert_eq!(axes[0].steps, 0);
    assert_eq!(axes[0].mean_abs_step, None);
    assert!(!axes[0].constant);
    assert_eq!(axes[1].mean_abs_step, Some(1.0));
    // Only one axis has a step: nothing dominates.
    assert_eq!(strongest(&axes, STRONGEST_RATIO), None);
}

// ---------------------------------------------------------------------------
// Pairs
// ---------------------------------------------------------------------------

#[test]
fn pairs_with_a_hole_are_counted_and_left_out() {
    // The crossplot rule: a pair with a member that is not finite is not plotted.
    let mut pairs = PairAccumulator::new(linear(0.0, 20.0), linear(0.0, 20.0), 4, None).unwrap();
    pairs
        .push(&[1.0, f64::NAN, 3.0], &[2.0, 2.0, f64::INFINITY])
        .unwrap();
    let summary = pairs.finish();
    assert_eq!(summary.pairs, 1);
    assert_eq!(summary.dropped, 2);
    assert_eq!(summary.share_matched, None);
}

#[test]
fn the_ratio_rule_counts_ratios_below_the_tolerance() {
    // S10: only the counting is here; the rule itself belongs to the layer
    // that checks sister properties.
    let mut pairs =
        PairAccumulator::new(linear(0.0, 20.0), linear(0.0, 20.0), 4, Some(CellRule::Below(0.95))).unwrap();
    pairs
        .push(&[10.0, 10.0, 10.0, 10.0, 0.0], &[9.0, 9.6, 10.0, 12.0, 5.0])
        .unwrap();
    let summary = pairs.finish();
    assert_eq!(summary.pairs, 5);
    assert_eq!(summary.undefined_ratio, 1, "a zero denominator has no ratio");
    assert_eq!(summary.matched, 1);
    assert_eq!(summary.share_matched, Some(0.25));
    assert_eq!(summary.ratio_rule, Some(CellRule::Below(0.95)));
    assert_eq!(summary.ratio_quantiles.len(), 5);
}

#[test]
fn perfectly_proportional_pairs_correlate_to_one_in_log() {
    let x: Vec<f64> = (1..=100).map(f64::from).collect();
    let y: Vec<f64> = x.iter().map(|v| 3.0 * v).collect();
    let log = || Scale::new(ScaleKind::Log10, 0.5, 1000.0, false).unwrap();

    let mut whole = PairAccumulator::new(log(), log(), 8, None).unwrap();
    whole.push(&x, &y).unwrap();
    close(whole.finish().correlation.unwrap(), 1.0, 1e-12);

    // In two halves, merged: forgetting the cross term would break it.
    let mut first = PairAccumulator::new(log(), log(), 8, None).unwrap();
    first.push(&x[..40], &y[..40]).unwrap();
    let mut second = PairAccumulator::new(log(), log(), 8, None).unwrap();
    second.push(&x[40..], &y[40..]).unwrap();
    first.merge(&second).unwrap();
    let merged = first.finish();
    close(merged.correlation.unwrap(), 1.0, 1e-12);
    assert_eq!(merged.pairs, 100);

    // Anti-proportional pairs correlate to minus one.
    let inverse: Vec<f64> = x.iter().map(|v| 1000.0 / v).collect();
    let mut against = PairAccumulator::new(log(), log(), 8, None).unwrap();
    against.push(&x, &inverse).unwrap();
    close(against.finish().correlation.unwrap(), -1.0, 1e-12);
}

#[test]
fn the_density_grid_puts_the_maximum_in_the_last_cell() {
    let mut grid = DensityGrid::new(linear(0.0, 1.0), linear(0.0, 1.0), 4).unwrap();
    assert!(grid.push(1.0, 1.0));
    assert_eq!(grid.count(3, 3), 1);
    assert!(grid.push(0.0, 0.0));
    assert_eq!(grid.count(0, 0), 1);
    assert!(!grid.push(2.0, 0.5));
    assert_eq!(grid.outside, 1);
    assert_eq!(grid.counts.iter().sum::<u64>(), 2);
}

#[test]
fn grids_of_two_sizes_do_not_merge() {
    let mut small = DensityGrid::new(linear(0.0, 1.0), linear(0.0, 1.0), 4).unwrap();
    let large = DensityGrid::new(linear(0.0, 1.0), linear(0.0, 1.0), 8).unwrap();
    assert_eq!(small.merge(&large), Err(PairRefusal::DifferentGrids));
    // The same size over other scales neither.
    let other = DensityGrid::new(linear(0.0, 2.0), linear(0.0, 1.0), 4).unwrap();
    assert_eq!(small.merge(&other), Err(PairRefusal::DifferentGrids));
    assert!(matches!(
        DensityGrid::new(linear(0.0, 1.0), linear(0.0, 1.0), 0),
        Err(PairRefusal::GridSize { .. })
    ));
}

#[test]
fn pairs_of_two_lengths_are_refused() {
    let mut pairs = PairAccumulator::new(linear(0.0, 1.0), linear(0.0, 1.0), 4, None).unwrap();
    assert_eq!(
        pairs.push(&[1.0, 2.0], &[1.0]),
        Err(PairRefusal::LengthsDiffer { x: 2, y: 1 })
    );
    assert!(matches!(
        PairAccumulator::new(linear(0.0, 1.0), linear(0.0, 1.0), 4, Some(CellRule::Below(f64::NAN))),
        Err(PairRefusal::Rule(TallyRefusal::ThresholdNotFinite { .. }))
    ));
}

// ---------------------------------------------------------------------------
// Cells
// ---------------------------------------------------------------------------

#[test]
fn the_tally_counts_per_row() {
    let counted = tally(&[0.5, 2.0, 0.9, f64::NAN, 3.0, 4.0], 2, &CellRule::Below(1.0)).unwrap();
    assert_eq!(counted.rows, 3);
    assert_eq!(counted.cols, 2);
    assert_eq!(counted.cells, 6);
    assert_eq!(counted.matched_per_row, vec![1, 1, 0]);
    assert_eq!(counted.present_per_row, vec![2, 1, 2]);
    assert_eq!(counted.matched, 2);
    assert_eq!(counted.absent, 1);
}

#[test]
fn runs_join_consecutive_rows_over_the_share() {
    let counted = tally(&[0.0, 0.0, 0.0, 0.0, 5.0, 5.0, 0.0, 0.0], 2, &CellRule::Below(1.0)).unwrap();
    assert_eq!(counted.matched_per_row, vec![2, 2, 0, 2]);
    assert_eq!(runs(&counted, 1.0).unwrap(), vec![0..2, 3..4]);
    // At half the cells, the same.
    assert_eq!(runs(&counted, 0.5).unwrap(), vec![0..2, 3..4]);
}

#[test]
fn a_row_of_holes_is_never_a_run() {
    // A hole is not an alert: the row of NaN between two alerts splits the run.
    let counted = tally(&[0.0, 0.0, f64::NAN, f64::NAN, 0.0, 0.0], 2, &CellRule::Below(1.0)).unwrap();
    assert_eq!(counted.present_per_row, vec![2, 0, 2]);
    assert_eq!(runs(&counted, 1.0).unwrap(), vec![0..1, 2..3]);
}

#[test]
fn a_ragged_matrix_is_refused() {
    assert_eq!(
        tally(&[1.0; 5], 2, &CellRule::Below(1.0)),
        Err(TallyRefusal::Ragged { values: 5, cols: 2 })
    );
    assert_eq!(tally(&[1.0; 4], 0, &CellRule::Below(1.0)), Err(TallyRefusal::NoColumn));
    assert!(matches!(
        tally(&[1.0; 4], 2, &CellRule::Outside { lo: 5.0, hi: 1.0 }),
        Err(TallyRefusal::BoundsReversed { .. })
    ));
    let counted = tally(&[1.0; 4], 2, &CellRule::Above(0.0)).unwrap();
    assert_eq!(runs(&counted, 0.0), Err(TallyRefusal::ShareOutOfRange { share: 0.0 }));
    assert_eq!(runs(&counted, 1.5), Err(TallyRefusal::ShareOutOfRange { share: 1.5 }));
}

#[test]
fn a_rule_says_nothing_of_a_value_that_is_not_a_number() {
    let outside = CellRule::Outside { lo: 1.0, hi: 2.0 };
    assert_eq!(outside.matches(0.5), Some(true));
    assert_eq!(outside.matches(1.5), Some(false));
    assert_eq!(outside.matches(2.5), Some(true));
    assert_eq!(outside.matches(f64::NAN), None);
    assert_eq!(CellRule::Above(1.0).matches(f64::INFINITY), None);
}
