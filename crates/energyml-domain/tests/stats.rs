//! Summarising an array, and the four ways a chart lies without failing.
//!
//! Every assertion here is about something that would still draw: a bar in the
//! wrong place, a mean pulled towards nothing by values that are not numbers, a
//! facies code binned as if it were a temperature, an extreme dropped off the
//! end of the last bin. A histogram has no failure mode that looks like a
//! failure.

use energyml_domain::stats::{CATEGORY_LIMIT, SAMPLE_LIMIT, Summary};
use energyml_domain::values::{Family, Values};

fn floats(values: &[f64]) -> Values {
    Values::Floating(values.to_vec())
}

#[test]
fn the_extremes_are_the_edges_and_the_last_bin_is_closed() {
    // Ten values 0…9, four bins: the edges are 0 and 9, and the 9 belongs to
    // the last bin. Computed the obvious way it lands at index 4 of a 4-bin
    // array - out of range, or silently dropped, and the value a person opened
    // the histogram to find is the one that vanishes.
    let summary = Summary::of(&floats(&[0., 1., 2., 3., 4., 5., 6., 7., 8., 9.]), 4);
    let histogram = summary.histogram.expect("a range has a histogram");
    assert_eq!(histogram.from, 0.0);
    assert_eq!(histogram.to, 9.0);
    assert_eq!(histogram.bins.iter().sum::<u64>(), 10);
    assert_eq!(histogram.bins.len(), 4);
    // 0…2.24 · 2.25…4.49 · 4.5…6.74 · 6.75…9
    assert_eq!(histogram.bins, vec![3, 2, 2, 3]);
}

#[test]
fn a_value_that_is_not_a_number_is_counted_rather_than_averaged() {
    // How the corpus says "no data here" - 785 250 of ALWYN's 987 387 grid
    // nodes. Counting them as values drags the mean towards nothing; skipping
    // them without saying so hides that four fifths of the array is empty.
    let summary = Summary::of(&floats(&[1.0, f64::NAN, 3.0, f64::INFINITY]), 8);
    assert_eq!(summary.count, 4);
    assert_eq!(summary.finite, 2);
    assert_eq!(summary.non_finite, 2);
    assert_eq!(summary.mean, Some(2.0));
    assert_eq!(summary.min, Some(1.0));
    assert_eq!(summary.max, Some(3.0));
    assert_eq!(summary.histogram.map(|h| h.bins.iter().sum::<u64>()), Some(2));
}

#[test]
fn an_array_of_nothing_but_holes_says_so_instead_of_drawing() {
    let summary = Summary::of(&floats(&[f64::NAN; 5]), 10);
    assert_eq!(summary.finite, 0);
    assert_eq!(summary.non_finite, 5);
    assert_eq!(summary.min, None);
    assert_eq!(summary.mean, None);
    // Not an empty histogram, which draws as a flat baseline and reads as "all
    // the values are here and they are all zero".
    assert!(summary.histogram.is_none());
}

#[test]
fn a_constant_array_is_one_bin_and_a_deviation_of_zero() {
    let summary = Summary::of(&floats(&[3.5; 100]), 30);
    let histogram = summary
        .histogram
        .expect("still a distribution, just a narrow one");
    assert_eq!(histogram.bins, vec![100]);
    assert_eq!(histogram.from, 3.5);
    assert_eq!(histogram.to, 3.5);
    assert_eq!(histogram.width(), 0.0);
    // Not `NaN`: the two-pass identity goes slightly negative on a constant
    // array, and the square root of that is a chart with no deviation at all
    // where the honest answer is zero.
    assert_eq!(summary.deviation, Some(0.0));
}

#[test]
fn a_discrete_array_is_counted_by_value_rather_than_binned() {
    // A facies property: ten answers, not a range. Binned over thirty
    // intervals it draws ten spikes with meaningless gaps between them.
    let facies = Values::Integer(vec![0, 0, 0, 1, 1, 2, 2, 2, 2, 9]);
    let summary = Summary::of(&facies, 30);
    assert!(summary.histogram.is_none());
    assert_eq!(summary.distinct, Some(4));
    let labels: Vec<&str> = summary.categories.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, vec!["0", "1", "2", "9"]);
    assert_eq!(summary.categories.iter().map(|c| c.count).sum::<u64>(), 10);
    // In value order, not in the order they were met: a legend that reorders
    // itself between two files reads as two different vocabularies.
    assert_eq!(summary.categories[3].value, Some(9.0));
}

#[test]
fn an_integer_array_with_many_values_is_a_range_like_any_other() {
    let counts: Vec<i64> = (0..(CATEGORY_LIMIT as i64 + 10)).collect();
    let summary = Summary::of(&Values::Integer(counts), 12);
    assert!(summary.categories.is_empty());
    assert_eq!(summary.distinct, None);
    assert_eq!(summary.histogram.map(|h| h.bins.len()), Some(12));
}

#[test]
fn floating_point_values_are_never_treated_as_categories() {
    // Three distinct doubles are not three categories: equality on doubles is
    // a bit pattern, and a "category" of 0.1 + 0.2 is a legend entry nobody
    // can match against their own data.
    let summary = Summary::of(&floats(&[1.0, 1.0, 2.0, 2.0, 3.0]), 6);
    assert!(summary.categories.is_empty());
    assert!(summary.histogram.is_some());
    assert_eq!(summary.distinct, None);
}

#[test]
fn booleans_name_both_halves_even_when_one_is_empty() {
    let summary = Summary::of(&Values::Boolean(vec![true; 4]), 10);
    assert_eq!(summary.family, Family::Boolean);
    let counts: Vec<(String, u64)> = summary
        .categories
        .iter()
        .map(|c| (c.label.clone(), c.count))
        .collect();
    // A run of four `true` and an array of which none is false are different
    // facts; a legend that omits the empty half hides which one this is.
    assert_eq!(counts, vec![("false".into(), 0), ("true".into(), 4)]);
    assert_eq!(summary.mean, Some(1.0));
    assert_eq!(summary.deviation, Some(0.0));
}

#[test]
fn strings_are_ranked_and_the_tail_is_a_remainder_that_is_named() {
    let mut values: Vec<String> = vec!["shale".into(); 5];
    values.extend(vec!["sand".into(); 3]);
    values.push("marl".into());
    let summary = Summary::of(&Values::Text(values), 10);
    assert_eq!(summary.distinct, Some(3));
    assert_eq!(summary.categories[0].label, "shale");
    assert_eq!(summary.categories[0].count, 5);
    assert_eq!(summary.categories[1].label, "sand");
    // No numbers to average, and none invented.
    assert_eq!(summary.mean, None);
    assert!(summary.histogram.is_none());
    assert_eq!(summary.other, 0);
}

#[test]
fn a_long_array_is_decimated_with_its_step_said_out_loud() {
    let values: Vec<f64> = (0..10_000).map(f64::from).collect();
    let summary = Summary::of(&floats(&values), 20);
    let sample = summary.sample.expect("something to plot");
    assert_eq!(sample.step, 5);
    assert!(sample.values.len() <= SAMPLE_LIMIT);
    // Regular and in order: the step is what lets a chart label the axis with
    // the array's own indices instead of pretending the sample is the array.
    assert_eq!(sample.values[0], 0.0);
    assert_eq!(sample.values[1], 5.0);
}

#[test]
fn a_short_array_is_its_own_sample() {
    let summary = Summary::of(&floats(&[1.0, 2.0, 3.0]), 4);
    let sample = summary.sample.expect("three values are three values");
    assert_eq!(sample.step, 1);
    assert_eq!(sample.values, vec![1.0, 2.0, 3.0]);
}

#[test]
fn a_hole_stays_a_hole_in_the_sample() {
    // Dropping the `NaN` would join the two sides of a gap in a log with a
    // straight line nobody measured, and it would draw as data.
    let summary = Summary::of(&floats(&[1.0, f64::NAN, 3.0]), 4);
    let sample = summary.sample.expect("a series");
    assert_eq!(sample.values.len(), 3);
    assert!(sample.values[1].is_nan());
}
