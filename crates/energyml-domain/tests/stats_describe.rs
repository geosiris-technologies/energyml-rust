//! Describing an array in steps: the same answer as in one call, a refusal
//! rather than a partial result, a sample that says it is one, and a scale that
//! is proposed from the decades the values span.
//!
//! Data is made here. Each expectation cites the decision of the specification
//! it follows (S7 seams, S8 three decades and percentiles).

use energyml_domain::color::Scaling;
use energyml_domain::stats::describe::Sampling;
use energyml_domain::stats::describe::{
    DescribeRefusal, Describer, Observation, Plan, SliceChunks, ValueChunks, describe,
};
use energyml_domain::stats::modes::StatMethods;
use energyml_domain::stats::scale::ScaleKind;

/// 120 values over `[30, 4]`, spread over four decades.
fn logarithmic_block() -> Vec<f64> {
    (0..120u64)
        .map(|k| 10f64.powf(((k * 7) % 120) as f64 / 25.0 - 1.0))
        .collect()
}

/// A source that must never be read.
struct NeverRead(Vec<u64>);

impl ValueChunks for NeverRead {
    fn dims(&self) -> &[u64] {
        &self.0
    }

    fn read(&self, _first: u64, _count: u64) -> energyml_domain::Result<Vec<f64>> {
        panic!("a description that is refused before reading must not read")
    }
}

/// A source that answers with fewer values than asked.
struct Short(Vec<u64>);

impl ValueChunks for Short {
    fn dims(&self) -> &[u64] {
        &self.0
    }

    fn read(&self, _first: u64, _count: u64) -> energyml_domain::Result<Vec<f64>> {
        Ok(Vec::new())
    }
}

fn has(
    description: &energyml_domain::stats::describe::Description,
    wanted: impl Fn(&Observation) -> bool,
) -> bool {
    description.observations.iter().any(wanted)
}

#[test]
fn a_description_in_steps_equals_one_in_one_call() {
    let values = logarithmic_block();
    let source = SliceChunks::new(&values, vec![30, 4]).unwrap();
    let mut describer = Describer::new(&source, Plan::default(), StatMethods::builtin()).unwrap();
    assert_eq!(describer.progress().total, 30);
    let mut steps = 0;
    loop {
        let progress = describer.step(&source, 7).unwrap();
        steps += 1;
        assert_eq!(progress, describer.progress());
        if progress.is_done() {
            break;
        }
    }
    assert_eq!(steps, 5, "30 slabs by 7 make five steps");
    let stepped = describer.finish().unwrap();
    let whole = describe(&values, &[30, 4], Plan::default()).unwrap();
    // The axes carry the last slab from one step to the next: without it their
    // steps along the slowest axis would differ.
    assert_eq!(stepped.axes, whole.axes);
    assert_eq!(stepped, whole);
    assert_eq!(whole.axes.len(), 2);
    assert_eq!(whole.sampling.visited, 120);
    assert_eq!(whole.sampling.total, 120);
}

#[test]
fn a_cancelled_description_is_not_a_partial_one() {
    let values = logarithmic_block();
    let source = SliceChunks::new(&values, vec![30, 4]).unwrap();
    let mut describer = Describer::new(&source, Plan::default(), StatMethods::builtin()).unwrap();
    let progress = describer.step(&source, 3).unwrap();
    assert_eq!((progress.done, progress.total), (3, 30));
    // A cancelled result presented as whole would be the worst kind of wrong:
    // it still draws.
    match describer.finish() {
        Err(DescribeRefusal::Cancelled { done }) => assert!((done - 10.0).abs() < 1e-9, "{done}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_sample_is_always_declared() {
    let values: Vec<f64> = (1..=100).map(f64::from).collect();
    let plan = Plan {
        sampling: Sampling::Regular { step: 4 },
        ..Plan::default()
    };
    let description = describe(&values, &[100], plan).unwrap();
    assert_eq!(description.sampling.visited, 25);
    assert_eq!(description.sampling.total, 100);
    assert!(has(&description, |o| matches!(
        o,
        Observation::Sampled {
            visited: 25,
            total: 100
        }
    )));
    // Neighbours are needed for the axes: they are not computed, and it says so.
    assert!(description.axes.is_empty());
    assert!(
        description
            .methods
            .iter()
            .any(|m| m.family == "axes" && m.id == "skipped")
    );
    // Whole arrays declare nothing of the kind.
    let whole = describe(&values, &[100], Plan::default()).unwrap();
    assert!(!has(&whole, |o| matches!(o, Observation::Sampled { .. })));
}

#[test]
fn a_slowest_window_visits_only_its_slabs() {
    let values: Vec<f64> = (0..40).map(f64::from).collect();
    let plan = Plan {
        sampling: Sampling::Slowest { first: 2, count: 3 },
        ..Plan::default()
    };
    let description = describe(&values, &[10, 4], plan).unwrap();
    assert_eq!(description.sampling.visited, 12);
    assert_eq!(description.sampling.total, 40);
    let moments = description.moments.unwrap();
    assert_eq!((moments.min, moments.max), (8.0, 19.0));
}

#[test]
fn four_decades_propose_the_log_scale() {
    // 0.11 to 6393 is 4.76 decades (the span of an UDAR resistivity); S8
    // proposes the log scale from three.
    let values: Vec<f64> = (0..200)
        .map(|i| 0.11 * (6393.0f64 / 0.11).powf(i as f64 / 199.0))
        .collect();
    let description = describe(&values, &[200], Plan::default()).unwrap();
    assert_eq!(description.scale.unwrap().kind(), ScaleKind::Log10);
    assert!(has(&description, |o| matches!(
        o,
        Observation::SpansDecades {
            proposed: ScaleKind::Log10,
            ..
        }
    )));
    assert!(!has(&description, |o| matches!(
        o,
        Observation::FileSaysLinear { .. }
    )));
    // The file says linear while the values span decades: said.
    let plan = Plan {
        file_scaling: Some(Scaling {
            logarithmic: false,
            ..Scaling::default()
        }),
        ..Plan::default()
    };
    let described = describe(&values, &[200], plan).unwrap();
    assert!(has(&described, |o| matches!(
        o,
        Observation::FileSaysLinear { .. }
    )));
    // The percentiles are the bounds, and the histogram is over them.
    let binned = description.binned.unwrap();
    assert_eq!(binned.bins(), 24);
    assert_eq!(description.quantiles.unwrap().values.len(), 5);
}

#[test]
fn a_linear_array_is_not_proposed_a_log_scale() {
    let values: Vec<f64> = (1..=100).map(f64::from).collect();
    let description = describe(&values, &[100], Plan::default()).unwrap();
    assert_eq!(description.scale.unwrap().kind(), ScaleKind::Linear);
    assert!(!has(&description, |o| matches!(
        o,
        Observation::SpansDecades { .. }
    )));
}

#[test]
fn a_log_asked_of_no_positive_value_is_refused_and_linear_stays() {
    let plan = Plan {
        scale: energyml_domain::stats::describe::ScaleChoice::Log10,
        ..Plan::default()
    };
    let description = describe(&[-3.0, -1.0, 0.0], &[3], plan).unwrap();
    assert!(has(&description, |o| matches!(o, Observation::LogRefused(_))));
    assert_eq!(description.scale.unwrap().kind(), ScaleKind::Linear);
}

#[test]
fn zeros_in_a_log_description_are_counted() {
    let plan = Plan {
        scale: energyml_domain::stats::describe::ScaleChoice::Log10,
        ..Plan::default()
    };
    let description = describe(&[0.0, 1.0, 10.0, 100.0, 1000.0], &[5], plan).unwrap();
    assert_eq!(description.scale.unwrap().kind(), ScaleKind::Log10);
    assert!(has(&description, |o| matches!(
        o,
        Observation::NonPositiveInLog { count: 1 }
    )));
}

#[test]
fn null_values_are_said_and_never_described() {
    // RESQML IntegerExternalArray.NullValue: counted apart, never averaged.
    let values = [1.0, -999.0, 3.0];
    let source = SliceChunks::new(&values, vec![3])
        .unwrap()
        .with_null_value(Some(-999));
    let mut describer = Describer::new(&source, Plan::default(), StatMethods::builtin()).unwrap();
    describer.step(&source, usize::MAX).unwrap();
    let description = describer.finish().unwrap();
    assert!(has(&description, |o| matches!(
        o,
        Observation::NullValues { count: 1, .. }
    )));
    assert_eq!(description.accumulator.null(), 1);
    assert_eq!(description.moments.unwrap().mean, 2.0);
}

#[test]
fn values_that_are_not_numbers_are_said() {
    let description = describe(&[1.0, f64::NAN, 3.0, f64::INFINITY], &[4], Plan::default()).unwrap();
    assert!(has(&description, |o| matches!(
        o,
        Observation::NotANumber { count: 2, .. }
    )));
    assert_eq!(description.moments.unwrap().mean, 2.0);
}

#[test]
fn values_outside_the_file_min_max_are_counted() {
    let plan = Plan {
        file_scaling: Some(Scaling {
            min_max: Some((0.0, 10.0)),
            ..Scaling::default()
        }),
        ..Plan::default()
    };
    let values: Vec<f64> = vec![1.0, 5.0, 9.0, 11.0, -2.0];
    let description = describe(&values, &[5], plan).unwrap();
    assert!(has(&description, |o| matches!(
        o,
        Observation::OutsideFileMinMax { count: 2, .. }
    )));
}

#[test]
fn an_unknown_method_is_refused_before_reading() {
    let source = NeverRead(vec![10]);
    let plan = Plan {
        mode_method: "nope".to_string(),
        ..Plan::default()
    };
    match Describer::new(&source, plan, StatMethods::builtin()) {
        Err(DescribeRefusal::UnknownMethod { family: "modes", id }) => assert_eq!(id, "nope"),
        Err(other) => panic!("expected an unknown method, got {other:?}"),
        Ok(_) => panic!("an unknown method must be refused"),
    }
    let plan = Plan {
        threshold_method: Some("nope".to_string()),
        ..Plan::default()
    };
    assert!(matches!(
        Describer::new(&source, plan, StatMethods::builtin()),
        Err(DescribeRefusal::UnknownMethod {
            family: "threshold",
            ..
        })
    ));
    // The same with an empty registry: not even the default plan is known.
    assert!(matches!(
        Describer::new(&source, Plan::default(), StatMethods::none()),
        Err(DescribeRefusal::UnknownMethod { family: "bins", .. })
    ));
}

#[test]
fn a_plan_out_of_range_is_refused_before_reading() {
    let source = NeverRead(vec![10, 2]);
    let refused = |plan: Plan| match Describer::new(&source, plan, StatMethods::builtin()) {
        Err(refusal) => refusal,
        Ok(_) => panic!("the plan must be refused"),
    };
    assert!(matches!(
        refused(Plan {
            bins: 0,
            ..Plan::default()
        }),
        DescribeRefusal::BinsOutOfRange { bins: 0, .. }
    ));
    assert!(matches!(
        refused(Plan {
            bins: 100_000,
            ..Plan::default()
        }),
        DescribeRefusal::BinsOutOfRange { bins: 100_000, .. }
    ));
    assert!(matches!(
        refused(Plan {
            quantiles: vec![0.5, 2.0],
            ..Plan::default()
        }),
        DescribeRefusal::QuantileOutOfRange { at } if at == 2.0
    ));
    assert!(matches!(
        refused(Plan {
            sampling: Sampling::Regular { step: 0 },
            ..Plan::default()
        }),
        DescribeRefusal::SamplingOutOfRange { .. }
    ));
    assert!(matches!(
        refused(Plan {
            sampling: Sampling::Slowest { first: 8, count: 3 },
            ..Plan::default()
        }),
        DescribeRefusal::SamplingOutOfRange { slabs: 10, .. }
    ));
    assert!(matches!(
        Describer::new(&NeverRead(Vec::new()), Plan::default(), StatMethods::builtin()),
        Err(DescribeRefusal::NoDimension)
    ));
}

#[test]
fn a_source_that_changes_shape_is_refused() {
    let values: Vec<f64> = (0..10).map(f64::from).collect();
    let source = SliceChunks::new(&values, vec![10]).unwrap();
    let mut describer = Describer::new(&source, Plan::default(), StatMethods::builtin()).unwrap();
    let other = SliceChunks::new(&values, vec![5, 2]).unwrap();
    match describer.step(&other, 1) {
        Err(DescribeRefusal::SourceChanged { expected, found }) => {
            assert_eq!(expected, vec![10]);
            assert_eq!(found, vec![5, 2]);
        }
        other => panic!("expected a change of shape, got {other:?}"),
    }
}

#[test]
fn a_short_read_is_refused() {
    let source = Short(vec![4, 2]);
    let mut describer = Describer::new(&source, Plan::default(), StatMethods::builtin()).unwrap();
    match describer.step(&source, 2) {
        Err(DescribeRefusal::ShortRead {
            first: 0,
            count: 2,
            expected: 4,
            got: 0,
        }) => {}
        other => panic!("expected a short read, got {other:?}"),
    }
}

#[test]
fn values_that_do_not_fill_the_dimensions_are_refused() {
    let refused = SliceChunks::new(&[1.0, 2.0, 3.0], vec![2, 2]);
    assert!(matches!(
        refused,
        Err(DescribeRefusal::ValuesDoNotFill {
            values: 3,
            expected: 4,
            ..
        })
    ));
}

#[test]
fn a_constant_array_has_no_scale_and_no_histogram_rather_than_a_guess() {
    let description = describe(&[5.0; 10], &[10], Plan::default()).unwrap();
    assert!(description.scale.is_none());
    assert!(description.binned.is_none());
    assert_eq!(description.moments.unwrap().deviation, 0.0);
    assert!(description.axes[0].constant);
    assert!(has(&description, |o| matches!(
        o,
        Observation::ConstantAlong { axis: 0 }
    )));
}

#[test]
fn a_seam_is_observed_with_its_position() {
    // S7: a step over ten times the median step is signalled. Nine steps of 1,
    // then one of 15.
    let mut values: Vec<f64> = (0..10).map(f64::from).collect();
    values.push(24.0);
    let description = describe(&values, &[11], Plan::default()).unwrap();
    assert!(has(
        &description,
        |o| matches!(o, Observation::Jump(j) if j.at == 10 && j.axis == 0)
    ));
}

#[test]
fn two_clusters_give_two_modes_and_a_threshold_in_a_description() {
    let mut values: Vec<f64> = (0..1000u64).map(|i| 1.0 + 0.001 * (i % 100) as f64).collect();
    values.extend((0..1000u64).map(|i| 100.0 + 0.1 * (i % 100) as f64));
    let n = values.len() as u64;
    let description = describe(&values, &[n], Plan::default()).unwrap();
    assert_eq!(description.modes.len(), 2, "{:?}", description.modes);
    let threshold = description.threshold.expect("two separated populations");
    assert!(threshold.at > 1.5 && threshold.at < 100.0, "{threshold:?}");
    assert!(has(&description, |o| matches!(
        o,
        Observation::Modes { count: 2 }
    )));
    assert!(has(&description, |o| matches!(o, Observation::Threshold(_))));
}
