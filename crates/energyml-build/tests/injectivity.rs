//! The injectivity index **recovered** from a test `injection-test`
//! synthesized - the recovered injectivity index is confronted with the parameter
//! that synthesized the pressure.
//!
//! Two independent sources of one fact: the `injectivity` parameter given to
//! the operation, and what `energyml_analysis::step_rate` reads back from the
//! channel values the plan wrote - through their JSON, their `TIME` strings
//! parsed here, never through the operation's own arithmetic. The Hall plot is
//! a third reading of the same `1 / II`.
//!
//! The settled criterion (`k`, `ε`) is declared in each test, and the
//! relaxation time is chosen so that the synthesized steps do or do not
//! settle by a margin a comment computes - the criterion is not tuned on the
//! model, the model is placed on either side of it.

use serde_json::Value;

use energyml_analysis::hall::hall;
use energyml_analysis::step_rate::{Series, Settle, StepRateError, injectivity, steps};
use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, Pieces, Ref, Source};

const GTH2: &str = "2d2d2d2d-0000-4000-8000-000000000002";
const COMPANY: &str = "2d2d2d2d-0000-4000-8000-000000000009";

const STEPS: [f64; 3] = [20.0, 40.0, 60.0];
const P0: f64 = 12.0;
const II: f64 = 10.0;

fn planned(response_minutes: f64) -> Plan {
    let pieces = Pieces::empty().with(
        "wellbore",
        Source::Object(Known {
            reference: Some(Ref::new(GTH2, "witsml21.Wellbore", "GTH-2")),
            ..Default::default()
        }),
    );
    let choices = Choices::new()
        .set("title", "GTH-2 - injection test")
        .set("source", "synthetic - a test of the injectivity diagnostic")
        .set("start", "2025-06-02T08:00:00Z")
        .set("steps", serde_json::json!(STEPS))
        .set("stepHours", 2.0)
        .set("sampleMinutes", 5)
        .set("staticPressure", P0)
        .set("injectivity", II)
        .set("responseMinutes", response_minutes)
        .set("reservoirTemperature", 30.0)
        .set("injectionTemperature", 12.0)
        .set("nominalPeriod", "day")
        .set("standardConditions", "15 degC 1 atm")
        .set("loggingCompany", serde_json::json!({ "Uuid": COMPANY, "QualifiedType": "eml23.BusinessAssociate", "Title": "Geosiris" }));
    let when = When {
        creation: "2026-09-28T00:00:00Z".into(),
        originator: "test".into(),
    };
    Registry::new()
        .plan("injection-test", &pieces, &choices, &Identity::derived(), &when)
        .unwrap_or_else(|why| panic!("a plan: {why}"))
}

/// Seconds since 1970 of `YYYY-MM-DDTHH:MM:SSZ` - the civil-to-days algorithm
/// of H. Hinnant, *chrono-Compatible Low-Level Date Algorithms*
/// (`days_from_civil`), so that the instants come from the strings written
/// and not from the operation's own clock.
fn seconds(text: &str) -> i64 {
    let number = |range: std::ops::Range<usize>| {
        text[range]
            .parse::<i64>()
            .unwrap_or_else(|_| panic!("`{text}` is no UTC date-time"))
    };
    assert!(
        text.len() == 20 && text.ends_with('Z'),
        "`{text}` is no UTC date-time"
    );
    let (y, m, d) = (number(0..4), number(5..7), number(8..10));
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400 + number(11..13) * 3_600 + number(14..16) * 60 + number(17..19)
}

/// The TIME, QINJ and BHP columns of the plan's channel set, read from the
/// log's JSON: instants in hours since the first, m3/h, bar.
fn columns(plan: &Plan) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let log = plan
        .objects
        .iter()
        .find(|one| one.qualified_type == "witsml21.Log")
        .expect("a log");
    let set = &log.json["ChannelSet"][0];
    let names: Vec<&str> = set["Channel"]
        .as_array()
        .expect("channels")
        .iter()
        .map(|one| one["Mnemonic"].as_str().expect("a mnemonic"))
        .collect();
    // A row is the index, then one value per channel in the order listed.
    let column = |mnemonic: &str| {
        1 + names
            .iter()
            .position(|one| *one == mnemonic)
            .unwrap_or_else(|| panic!("a channel `{mnemonic}` in {names:?}"))
    };
    let (time, rate, pressure) = (column("TIME"), column("QINJ"), column("BHP"));
    let text = set["Data"]["Data"].as_str().expect("the values inline");
    let rows: Vec<Vec<Value>> = serde_json::from_str(text).expect("a JSON array of rows");
    let origin = seconds(rows[0][time].as_str().expect("a date-time"));
    let hours = rows
        .iter()
        .map(|row| (seconds(row[time].as_str().expect("a date-time")) - origin) as f64 / 3_600.0)
        .collect();
    let rates = rows
        .iter()
        .map(|row| row[rate].as_f64().expect("a rate"))
        .collect();
    let pressures = rows
        .iter()
        .map(|row| row[pressure].as_f64().expect("a pressure"))
        .collect();
    (hours, rates, pressures)
}

#[test]
fn the_index_read_from_the_settled_steps_is_the_one_that_synthesized_them() {
    // τ = 10 min on steps of 2 h sampled every 5 min. Each step starts 2 bar
    // (20 m3/h ÷ 10) from its target; the window of the last 4 samples opens
    // 100 min into the step, where |dp/dt| = 2 / 10 × e^(−10) bar/min
    // ≈ 5.4e-4 bar/h - under ε = 1e-3 bar/h by a factor of two, and the next
    // sample halves it again. The last sample still lies 2 × e^(−11.5)
    // ≈ 2e-5 bar short of its target, the same for the first two steps, so
    // the slope barely sees it.
    // To break: regress q against Δp, take 1 / intercept, or read the static
    // pressure from the first step - the index moves off 10.
    let plan = planned(10.0);
    let (hours, rates, pressures) = columns(&plan);
    let series = Series::new(&hours, &rates, &pressures).expect("a series");
    let settle = Settle {
        window: 4,
        tolerance: 1e-3,
    };
    let found = injectivity(&series, &settle, P0).expect("an index");
    assert_eq!(
        found
            .points
            .iter()
            .map(|one| (one.rank, one.rate))
            .collect::<Vec<_>>(),
        [(1, 20.0), (2, 40.0), (3, 60.0)]
    );
    assert!(found.set_aside.is_empty(), "{:?}", found.set_aside);
    assert!(
        (found.index - II).abs() < 1e-3,
        "recovered {} where {II} synthesized",
        found.index
    );
    assert!(
        found.fit.intercept.abs() < 1e-3,
        "Δp is q / II through the origin: {:?}",
        found.fit
    );
    for ratio in &found.ratios {
        let ratio = ratio.as_ref().expect("a ratio");
        assert!(
            (ratio.index - II).abs() < 1e-2,
            "step {}: {}",
            ratio.rank,
            ratio.index
        );
    }
}

#[test]
fn a_test_whose_steps_never_settle_is_refused_step_by_step() {
    // τ = 120 min, the length of a step: 100 min in, |dp/dt| is still
    // 2 / 120 × e^(−100/120) bar/min ≈ 0.43 bar/h, four hundred times ε.
    // Every step is refused by its rank; no index is read.
    // To break: read an unsettled step at its last sample - an index comes
    // out, from pressures that stopped short of their targets.
    let plan = planned(120.0);
    let (hours, rates, pressures) = columns(&plan);
    let series = Series::new(&hours, &rates, &pressures).expect("a series");
    let settle = Settle {
        window: 4,
        tolerance: 1e-3,
    };
    let all = steps(&series, &settle).expect("steps");
    assert_eq!(all.len(), 3);
    for step in &all {
        assert!(
            matches!(step.verdict, Err(StepRateError::NotSettled { rank, .. }) if rank == step.rank),
            "{step:?}"
        );
    }
    assert_eq!(
        injectivity(&series, &settle, P0),
        Err(StepRateError::TooFewSettled {
            settled: 0,
            set_aside: vec![1, 2, 3]
        })
    );
}

#[test]
fn the_hall_slope_of_an_instant_response_is_the_inverse_of_the_index() {
    // τ = 0: from the second sample on, every pressure is P0 + q / II, so
    // each trapezoid of Δp is the trapezoid of q divided by II - the Hall
    // line from sample 2 to the last has slope 1 / II exactly. (The first
    // sample is the static pressure at the first rate: the first interval is
    // not on the line, which is why it is left out.)
    // To break: integrate p instead of Δp, or a left rectangle for one of the
    // two integrals and not the other.
    let plan = planned(0.0);
    let (hours, rates, pressures) = columns(&plan);
    let plot = hall(&Series::new(&hours, &rates, &pressures).expect("a series"), P0).expect("a Hall plot");
    let last = plot.volume.len() - 1;
    let slope = (plot.pressure_time[last] - plot.pressure_time[1]) / (plot.volume[last] - plot.volume[1]);
    assert!((slope - 1.0 / II).abs() < 1e-12, "{slope} where 1 / {II}");
    // The volume injected is 20, 40, 60 m3/h for 2 h each, 240 m3 - but the
    // sample at an instant carries the rate that starts there, so the
    // interval that ends each step is credited the mean of two rates: the
    // trapezoid the module states counts (20 / 2) × (5 / 60) m3 too much at
    // each of the two jumps. Said by the rule, checked here.
    assert!(
        (plot.volume[last] - (240.0 + 2.0 * 10.0 * 5.0 / 60.0)).abs() < 1e-9,
        "{}",
        plot.volume[last]
    );
}
