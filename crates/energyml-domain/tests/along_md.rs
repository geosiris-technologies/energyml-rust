//! `energyml_domain::along_md`: the gap rule, brackets, the stepper and the
//! interpolations. Decisions S7 and S8 (ADR-0027); the behaviours come from the
//! design sheet of the well view (fiche 03, section 4.3 and 10), not from a file.
//!
//! Every value is fabricated; nothing is read from disk.

use std::f64::consts::{PI, TAU};

use energyml_domain::along_md::{
    ANGULAR, Applies, Bracket, BracketRefusal, GapRule, InterpChoice, InterpNote, InterpRefusal,
    Interpolations, Interpolator, LINEAR, LOG_LINEAR, MdStepper, NEAREST, Nature, PREVIOUS, StepMode,
    StepRefusal, bracket, default_interpolation, interpolate_row, median_step, nearest_node, union_axis,
};

fn builtin(id: &str) -> Box<dyn Interpolator> {
    // A registry owns its methods; ask a fresh one for the answer and keep the
    // test free of the private types.
    struct Via(Interpolations, String);
    impl Interpolator for Via {
        fn id(&self) -> &'static str {
            "via"
        }
        fn applies(&self, nature: &Nature) -> Applies {
            self.0.get(&self.1).expect("a built-in id").applies(nature)
        }
        fn between(&self, a: f64, b: f64, t: f64) -> Result<f64, InterpRefusal> {
            self.0.get(&self.1).expect("a built-in id").between(a, b, t)
        }
    }
    Box::new(Via(Interpolations::builtin(), id.to_string()))
}

fn rule() -> GapRule {
    GapRule::of_axis(&[0.0, 1.0, 2.0]).expect("a good axis")
}

// ---- F11: the gap rule ----

/// S7 (ADR-0027): a gap wider than 3 median steps is never bridged.
/// Break it: `GAP_RATIO = 2.0`.
#[test]
fn the_gap_rule_is_three_median_steps() {
    let nodes = [0.0, 1.0, 2.0, 3.0, 10.0];
    assert_eq!(median_step(&nodes), Ok(1.0));
    let rule = GapRule::of_axis(&nodes).unwrap();
    assert_eq!(rule.max_gap(), 3.0);
    assert!(rule.bridges(3.0));
    assert!(!rule.bridges(3.01));
    assert!(!rule.bridges(f64::NAN));
    assert!(!rule.bridges(-1.0));
}

/// S7: a caller may be stricter, never looser. Break it: `max` for `min`.
#[test]
fn a_caller_may_be_stricter_never_looser() {
    let rule = rule();
    assert_eq!(rule.stricter(2.0).max_gap(), 2.0);
    assert_eq!(rule.stricter(10.0).max_gap(), 3.0);
    assert_eq!(rule.stricter(f64::NAN).max_gap(), 3.0);
    assert_eq!(rule.stricter(-1.0).max_gap(), 3.0);
}

/// Fiche 03 section 10: MDs must strictly increase, nothing is sorted silently.
/// Break it: sort the axis before checking.
#[test]
fn descending_or_repeated_depths_are_refused() {
    assert_eq!(
        median_step(&[0.0, 1.0, 1.0, 2.0]),
        Err(StepRefusal::NotIncreasing { at: 2 })
    );
    assert_eq!(
        bracket(&[0.0, 1.0, 1.0, 2.0], 0.5, 0.0),
        Err(BracketRefusal::NotIncreasing { at: 2 })
    );
    assert_eq!(median_step(&[1.0]), Err(StepRefusal::TooFewNodes { nodes: 1 }));
    assert_eq!(
        median_step(&[0.0, f64::NAN]),
        Err(StepRefusal::NodeNotFinite { at: 1 })
    );
}

// ---- F12: brackets ----

/// Fiche 03 section 10. Break it: drop the tolerance.
#[test]
fn a_depth_on_a_node_brackets_that_node_alone() {
    let b = bracket(&[3000.0, 3001.0, 3002.0], 3001.0 + 1e-9, 1e-6).unwrap();
    assert_eq!(
        b,
        Bracket {
            lower: 1,
            upper: 1,
            t: 0.0
        }
    );
    assert_eq!(b.on_node(), Some(1));
}

/// Fiche 03 section 10. Break it: swap `lower` and `upper`.
#[test]
fn a_depth_between_two_nodes_gives_the_fraction() {
    let b = bracket(&[3000.0, 3001.0, 3002.0], 3001.4, 1e-6).unwrap();
    assert_eq!((b.lower, b.upper), (1, 2));
    assert!((b.t - 0.4).abs() < 1e-9);
    assert_eq!(b.on_node(), None);
}

/// Fiche 03 section 10. Break it: clamp instead of refusing.
#[test]
fn a_depth_outside_the_support_is_refused_with_its_bounds() {
    assert_eq!(
        bracket(&[3000.0, 3001.0, 3002.0], 2999.0, 1e-6),
        Err(BracketRefusal::Outside {
            md: 2999.0,
            from: 3000.0,
            to: 3002.0
        })
    );
    assert_eq!(
        bracket(&[3000.0], 3000.5, 1e-6),
        Err(BracketRefusal::Outside {
            md: 3000.5,
            from: 3000.0,
            to: 3000.0
        })
    );
}

/// Fiche 03 section 4.3: on a tie, the shallower node. Break it: take the deeper.
#[test]
fn the_nearest_node_on_a_tie_is_the_shallower() {
    assert_eq!(nearest_node(&[10.0, 20.0], 15.0), Some(0));
    // A descending axis is kept (the desktop's `shared::nearest` does).
    assert_eq!(nearest_node(&[20.0, 10.0], 15.0), Some(1));
    assert_eq!(nearest_node(&[10.0, 20.0], f64::NAN), None);
}

/// Fiche 03 section 4.2 (`master_axis`). Break it: a tolerance of zero.
#[test]
fn the_union_of_two_axes_merges_close_nodes() {
    let a = [0.0, 1.0, 2.0];
    let b = [1.0 + 1e-9, 2.5];
    assert_eq!(union_axis(&[&a, &b], 1e-6).unwrap(), vec![0.0, 1.0, 2.0, 2.5]);
    assert_eq!(
        union_axis(&[&a, &b], -1.0),
        Err(StepRefusal::BadTolerance { tolerance: -1.0 })
    );
}

// ---- F13: the stepper ----

/// Fiche 03 section 5.2: the first notch goes to the node strictly beyond.
/// Break it: start from the nearest node (the results become 3 and 0).
#[test]
fn nodes_mode_goes_to_the_next_node_strictly() {
    let s = MdStepper::new(vec![0.0, 1.0, 2.0, 3.0], StepMode::Nodes).unwrap();
    let up = s.step(1.6, 1);
    assert_eq!((up.md, up.on_node), (2.0, Some(2)));
    let down = s.step(1.4, -1);
    assert_eq!((down.md, down.on_node), (1.0, Some(1)));
    // On a node, a notch moves by one node.
    assert_eq!(s.step(1.0, 1).md, 2.0);
}

/// Fiche 03 section 4.2 (`snap_to_nodes`). Break it: do not settle on the node.
#[test]
fn fine_mode_snaps_on_a_node_within_tolerance() {
    let s = MdStepper::new(
        vec![0.0, 0.5000000001, 2.0],
        StepMode::Fine {
            step: 0.5,
            anchor: 0.0,
        },
    )
    .unwrap();
    let r = s.step(0.0, 1);
    assert_eq!((r.md, r.on_node), (0.5000000001, Some(1)));
    // Between grid points the first notch goes to the grid point beyond.
    assert_eq!(s.step(0.7, 1).md, 1.0);
}

/// Fiche 03 section 4.2: at most 10^6 fine steps. Break it: drop the limit.
#[test]
fn too_many_fine_steps_are_refused() {
    let r = MdStepper::new(
        vec![0.0, 2e6],
        StepMode::Fine {
            step: 1.0,
            anchor: 0.0,
        },
    );
    assert_eq!(
        r,
        Err(StepRefusal::TooManySteps {
            count: 2_000_000,
            limit: 1_000_000
        })
    );
}

/// Fiche 03 section 4.2 (`clamped`). Break it: never say `clamped`.
#[test]
fn stepping_past_the_end_is_clamped_and_said() {
    let s = MdStepper::new(vec![0.0, 1.0, 2.0, 3.0], StepMode::Nodes).unwrap();
    let r = s.step(3.0, 5);
    assert_eq!(r.md, 3.0);
    assert!(r.clamped);
    assert!(!s.step(1.0, 1).clamped);
    assert_eq!(s.step(f64::NAN, 1).md, 0.0);
}

// ---- F14: interpolations ----

/// Fiche 03 section 10. Break it: interpolate linearly.
#[test]
fn log_linear_halfway_is_the_geometric_mean() {
    let m = builtin(LOG_LINEAR);
    assert!((m.between(10.0, 1000.0, 0.5).unwrap() - 100.0).abs() < 1e-9);
}

/// R11. Break it: take the absolute value.
#[test]
fn log_linear_refuses_a_non_positive_value() {
    let m = builtin(LOG_LINEAR);
    assert_eq!(
        m.between(-1.0, 10.0, 0.5),
        Err(InterpRefusal::NonPositive { value: -1.0 })
    );
}

/// Fiche 03 section 4.3. Break it: do not bring the difference back (180 degrees).
#[test]
fn angular_goes_the_short_way_round() {
    let m = builtin(ANGULAR);
    let v = m.between(350f64.to_radians(), 10f64.to_radians(), 0.5).unwrap();
    let from_zero = v.rem_euclid(TAU).min(TAU - v.rem_euclid(TAU));
    assert!(from_zero < 1e-12, "{v}");
}

/// R12. Break it: choose a direction.
#[test]
fn angular_refuses_opposite_angles() {
    let m = builtin(ANGULAR);
    assert!(matches!(
        m.between(0.0, PI, 0.5),
        Err(InterpRefusal::OppositeAngles { .. })
    ));
}

/// R10. Break it: do not read `categorical`.
#[test]
fn linear_is_refused_on_a_category() {
    let nature = Nature {
        categorical: true,
        ..Nature::default()
    };
    let m = builtin(LINEAR);
    assert_eq!(
        interpolate_row(&[1.0], &[2.0], 0.5, 1.0, &rule(), m.as_ref(), &nature),
        Err(InterpRefusal::Categorical)
    );
}

/// Fiche 03 section 4.3: a NaN cuts the curve. Break it: take the other value.
#[test]
fn a_missing_neighbour_is_never_bridged() {
    let m = builtin(LINEAR);
    let r = interpolate_row(
        &[1.0, f64::NAN],
        &[3.0, 5.0],
        0.5,
        1.0,
        &rule(),
        m.as_ref(),
        &Nature::default(),
    )
    .unwrap();
    assert_eq!(r.values[0], 2.0);
    assert!(r.values[1].is_nan());
    assert_eq!(r.missing, 1);
    assert_eq!(r.refused, 0);
}

/// S7, R14. Break it: do not test the gap.
#[test]
fn a_gap_wider_than_allowed_is_not_bridged() {
    let m = builtin(LINEAR);
    let r = interpolate_row(
        &[1.0, 2.0],
        &[3.0, 4.0],
        0.5,
        4.0,
        &rule(),
        m.as_ref(),
        &Nature::default(),
    )
    .unwrap();
    assert!(r.values.iter().all(|v| v.is_nan()));
    assert_eq!(
        r.first_refusal,
        Some(InterpRefusal::GapTooWide { gap: 4.0, max: 3.0 })
    );
}

/// A node is not a bridge. Break it: test the gap before `t == 0`.
#[test]
fn on_a_node_the_value_is_the_node_s_even_across_a_wide_gap() {
    let m = builtin(LINEAR);
    let r = interpolate_row(
        &[1.0, 2.0],
        &[3.0, 4.0],
        0.0,
        100.0,
        &rule(),
        m.as_ref(),
        &Nature::default(),
    )
    .unwrap();
    assert_eq!(r.values, vec![1.0, 2.0]);
    assert_eq!(r.first_refusal, None);
}

/// Fiche 03 section 4.3: only `previous` has a sense. Break it: accept `nearest`.
#[test]
fn intervals_hold_only() {
    let nature = Nature {
        intervals: true,
        ..Nature::default()
    };
    for id in [NEAREST, LINEAR] {
        let m = builtin(id);
        assert_eq!(
            interpolate_row(&[1.0], &[2.0], 0.5, 1.0, &rule(), m.as_ref(), &nature),
            Err(InterpRefusal::IntervalsHoldOnly),
            "{id}"
        );
    }
    let m = builtin(PREVIOUS);
    let r = interpolate_row(&[1.0], &[2.0], 0.7, 1.0, &rule(), m.as_ref(), &nature).unwrap();
    assert_eq!(r.values, vec![1.0]);
}

/// Q10: linear on an angle is accepted, with a note. Break it: refuse it.
#[test]
fn linear_on_an_angle_is_accepted_with_a_note() {
    let nature = Nature {
        angle: true,
        ..Nature::default()
    };
    let m = builtin(LINEAR);
    let r = interpolate_row(&[1.0], &[2.0], 0.5, 1.0, &rule(), m.as_ref(), &nature).unwrap();
    assert_eq!(r.note, Some(InterpNote::AngleTakenAsLinear));
}

/// Fiche 03 section 4.3. Break it: always return `linear`.
#[test]
fn the_default_method_is_read_from_the_property() {
    let n = Nature::default();
    let with = |f: fn(&mut Nature)| {
        let mut x = n;
        f(&mut x);
        x
    };
    assert_eq!(
        default_interpolation(&with(|x| x.intervals = true), false),
        (PREVIOUS, InterpChoice::Intervals)
    );
    assert_eq!(
        default_interpolation(&with(|x| x.categorical = true), false),
        (NEAREST, InterpChoice::Category)
    );
    assert_eq!(
        default_interpolation(&with(|x| x.angle = true), false),
        (ANGULAR, InterpChoice::Angle)
    );
    assert_eq!(
        default_interpolation(&n, true),
        (LOG_LINEAR, InterpChoice::LogScale)
    );
    assert_eq!(default_interpolation(&n, false), (LINEAR, InterpChoice::Measure));
}

/// ARCHITECTURE.md, registries: the caller's additions are tried first. Break
/// it: append instead of inserting at the head.
#[test]
fn an_addition_of_the_caller_is_tried_first() {
    struct Always42;
    impl Interpolator for Always42 {
        fn id(&self) -> &'static str {
            "linear"
        }
        fn applies(&self, _: &Nature) -> Applies {
            Applies::Yes
        }
        fn between(&self, _: f64, _: f64, _: f64) -> Result<f64, InterpRefusal> {
            Ok(42.0)
        }
    }
    let mut registry = Interpolations::builtin();
    assert_eq!(
        registry.ids(),
        vec![NEAREST, PREVIOUS, LINEAR, LOG_LINEAR, ANGULAR]
    );
    registry.push(Box::new(Always42));
    assert_eq!(registry.get("linear").unwrap().between(0.0, 1.0, 0.5), Ok(42.0));
    assert_eq!(registry.ids().len(), 5, "an id is listed once");
    assert!(registry.get("unknown").is_none());
}
