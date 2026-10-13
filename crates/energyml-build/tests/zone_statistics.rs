//! `zone-statistics` - a property's statistics per zone and per stratigraphic
//! unit, as an
//! `eml23.ColumnBasedTable` read back into the generated type.
//!
//! The expected rows are recomputed **here, by a naive loop** over the values
//! and the keys - never by `energyml_domain::stats`, which is what the
//! operation uses.
//!
//! # What would have to break to make it turn red
//!
//! Averaging a null (or counting it as a value); pairing the keys and the
//! values shifted by one rank; rounding a zone code; filing in a
//! group the elements without a zone; accepting zones laid on other
//! elements; reading the unit of an interval anywhere other than in its frame.

use energyml_build::operation::{Choices, Identity, Plan, Registry, When};
use energyml_build::pieces::{Known, KnownProperty, Pieces, Ref, Source};
use energyml_domain::provenance::Parameter;
use energyml_domain::values::Values;
use energyml_eml_2_3 as eml;

fn when() -> When {
    When {
        creation: "2026-09-25T00:00:00Z".into(),
        originator: "test".into(),
    }
}

const GRID: &str = "11111111-0000-4000-8000-000000000001";
const FRAME: &str = "22222222-0000-4000-8000-000000000002";

fn porosity_kind() -> Ref {
    Ref::new(
        "cfe9293f-d5a9-486d-815a-a957cace90b6",
        "eml23.PropertyKind",
        "porosity",
    )
}

fn field(over: &str, on: &str, values: Vec<f64>) -> Source {
    Source::Field {
        over: over.into(),
        on: on.into(),
        values,
        uom: Some("m3/m3".into()),
    }
}

/// A grid of the package, named `grid`, with a porosity and a zone code on
/// each of its twelve cells.
fn on_grid(values: Vec<f64>, zones: Source) -> Pieces {
    Pieces::empty()
        .with(
            "grid",
            Source::Object(Known {
                reference: Some(Ref::new(GRID, "resqml22.IjkGridRepresentation", "The grid")),
                ..Default::default()
            }),
        )
        .with("values", field("grid", "cells", values))
        .with("zones", zones)
}

fn run(pieces: &Pieces, choices: Choices) -> Result<Plan, energyml_build::operation::Error> {
    Registry::new().plan("zone-statistics", pieces, &choices, &Identity::derived(), &when())
}

/// The table the plan writes, through the road `apply` takes.
fn table(plan: &Plan) -> eml::ColumnBasedTable {
    assert_eq!(plan.objects.len(), 1);
    let schemas = energyml_schemas::all();
    let document = energyml_core::json::read_value(&plan.objects[0].json, &schemas)
        .unwrap_or_else(|why| panic!("the table does not read back: {why}"));
    document
        .root
        .as_reflect()
        .downcast_ref::<eml::ColumnBasedTable>()
        .expect("an eml 2.3 ColumnBasedTable")
        .clone()
}

fn column<'a>(table: &'a eml::ColumnBasedTable, title: &str) -> &'a eml::Column {
    table
        .column
        .iter()
        .chain(&table.key_column)
        .find(|c| c.title.as_deref() == Some(title))
        .unwrap_or_else(|| panic!("no column `{title}`"))
}

fn reals(table: &eml::ColumnBasedTable, title: &str) -> Vec<f64> {
    match &column(table, title).values {
        Some(eml::AbstractValueArray::FloatingPointXmlArray(array)) => array.values.clone(),
        other => panic!("`{title}` is no column of doubles: {other:?}"),
    }
}

fn integers(table: &eml::ColumnBasedTable, title: &str) -> Vec<i64> {
    match &column(table, title).values {
        Some(eml::AbstractValueArray::IntegerXmlArray(array)) => array.values.clone(),
        other => panic!("`{title}` is no column of integers: {other:?}"),
    }
}

fn texts(table: &eml::ColumnBasedTable, title: &str) -> Vec<String> {
    match &column(table, title).values {
        Some(eml::AbstractValueArray::StringXmlArray(array)) => array.values.clone(),
        other => panic!("`{title}` is no column of text: {other:?}"),
    }
}

fn reported(plan: &Plan, title: &str) -> f64 {
    plan.activity
        .parameters
        .iter()
        .find_map(|p| match p {
            Parameter::Double { title: t, value, .. } if t == title => Some(*value),
            _ => None,
        })
        .unwrap_or_else(|| panic!("`{title}` is not reported"))
}

/// What the table should say, by a naive loop: for each key in ascending
/// order, the count of numbers, the count of nulls, min, max, mean and the
/// population deviation.
fn naive(values: &[f64], keys: &[Option<i64>]) -> Vec<(i64, i64, i64, f64, f64, f64, f64)> {
    let mut distinct: Vec<i64> = keys.iter().flatten().copied().collect();
    distinct.sort();
    distinct.dedup();
    distinct
        .into_iter()
        .map(|key| {
            let mut numbers = Vec::new();
            let mut nulls = 0;
            for i in 0..values.len() {
                if keys[i] == Some(key) {
                    if values[i].is_nan() {
                        nulls += 1
                    } else {
                        numbers.push(values[i])
                    }
                }
            }
            let n = numbers.len() as f64;
            let mean = numbers.iter().sum::<f64>() / n;
            let deviation = (numbers.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / n).sqrt();
            let min = numbers.iter().copied().fold(f64::INFINITY, f64::min);
            let max = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            (key, numbers.len() as i64, nulls, min, max, mean, deviation)
        })
        .collect()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * b.abs().max(1.0)
}

#[test]
fn statistics_per_zone_agree_with_a_naive_loop_and_nulls_are_counted_not_averaged() {
    let values = vec![
        0.10,
        0.20,
        f64::NAN,
        0.30,
        0.25,
        0.05,
        f64::NAN,
        0.40,
        0.15,
        0.35,
        0.22,
        0.18,
    ];
    let zones = vec![3.0, 1.0, 3.0, 3.0, 1.0, f64::NAN, 1.0, 7.0, 3.0, 7.0, 1.0, 3.0];
    let keys: Vec<Option<i64>> = zones
        .iter()
        .map(|z: &f64| (!z.is_nan()).then_some(*z as i64))
        .collect();
    let pieces = on_grid(values.clone(), field("grid", "cells", zones));
    let plan = run(&pieces, Choices::new().set("propertyKind", porosity_kind().dor())).expect("planned");
    let table = table(&plan);

    let expected = naive(&values, &keys);
    assert_eq!(
        integers(&table, "zone"),
        expected.iter().map(|row| row.0).collect::<Vec<_>>()
    );
    assert_eq!(
        integers(&table, "value count"),
        expected.iter().map(|row| row.1).collect::<Vec<_>>()
    );
    assert_eq!(
        integers(&table, "null count"),
        expected.iter().map(|row| row.2).collect::<Vec<_>>()
    );
    for (title, pick) in [
        ("minimum", 3usize),
        ("maximum", 4),
        ("mean", 5),
        ("standard deviation", 6),
    ] {
        let found = reals(&table, title);
        for (row, want) in expected.iter().enumerate() {
            let want = [row_f(want, 3), row_f(want, 4), row_f(want, 5), row_f(want, 6)][pick - 3];
            assert!(
                close(found[row], want),
                "`{title}` row {row}: {} against {want}",
                found[row]
            );
        }
    }
    // The kind of the values, and their unit, on the statistics columns; the
    // facet eml 2.3 has for a mean.
    let mean = column(&table, "mean");
    assert_eq!(mean.property_kind.title, "porosity");
    assert_eq!(
        mean.facet.first().map(|f| f.kind.clone()),
        Some(eml::FacetKind::Statistics)
    );
    assert_eq!(reported(&plan, "nullCount"), 2.0);
    assert_eq!(
        reported(&plan, "ungroupedCount"),
        1.0,
        "the element with no zone is in no group"
    );
    assert_eq!(reported(&plan, "groupCount"), 3.0);
}

fn row_f(row: &(i64, i64, i64, f64, f64, f64, f64), at: usize) -> f64 {
    match at {
        3 => row.3,
        4 => row.4,
        5 => row.5,
        _ => row.6,
    }
}

#[test]
fn zones_that_are_not_the_values_elements_are_refused() {
    let values = vec![1.0; 4];
    let on_nodes = on_grid(values.clone(), field("grid", "nodes", vec![1.0; 4]));
    let refused = run(
        &on_nodes,
        Choices::new().set("propertyKind", porosity_kind().dor()),
    )
    .expect_err("nodes against cells");
    assert_eq!(refused.refusals()[0].slot.as_deref(), Some("zones"), "{refused}");

    let short = on_grid(values.clone(), field("grid", "cells", vec![1.0; 3]));
    let refused = run(&short, Choices::new().set("propertyKind", porosity_kind().dor()))
        .expect_err("three keys for four values");
    assert!(
        refused.message().contains("3 group key(s) for 4 value(s)"),
        "{refused}"
    );

    let fraction = on_grid(values, field("grid", "cells", vec![1.0, 2.5, 1.0, 1.0]));
    let refused = run(
        &fraction,
        Choices::new().set("propertyKind", porosity_kind().dor()),
    )
    .expect_err("2.5 is no code");
    assert!(refused.message().contains("element 1 holds 2.5"), "{refused}");
}

#[test]
fn a_kind_is_required_when_the_values_do_not_state_one() {
    let pieces = on_grid(vec![1.0, 2.0], field("grid", "cells", vec![1.0, 1.0]));
    let refused = run(&pieces, Choices::new()).expect_err("no kind");
    assert_eq!(refused.refusals()[0].slot.as_deref(), Some("propertyKind"));
}

/// A frame of five intervals naming three units - the second interval none -
/// and an integer property on those intervals, whose null is -999.
fn well(values: Vec<i64>, units: Vec<Option<Ref>>, supporting: &str) -> Pieces {
    let frame = Known {
        reference: Some(Ref::new(FRAME, "resqml22.WellboreFrameRepresentation", "SP1")),
        interval_units: Some(units),
        ..Default::default()
    };
    let property = Known {
        reference: Some(Ref::new(
            "33333333-0000-4000-8000-000000000003",
            "resqml22.DiscreteProperty",
            "Classe",
        )),
        property: Some(KnownProperty {
            supporting: Some(Ref::new(
                supporting,
                "resqml22.WellboreFrameRepresentation",
                "SP1",
            )),
            indexable: "intervals".into(),
            value_count: vec![1],
            timed: false,
            realizations: false,
            uom: Some("Euc".into()),
            kind: Some(Ref::new(
                "5f78f66a-ed1b-4827-a868-beb989febb31",
                "eml23.PropertyKind",
                "code",
            )),
            lookup: None,
            null_value: Some(-999),
            facets: Vec::new(),
            values: Ok(Values::Integer(values)),
        }),
        ..Default::default()
    };
    Pieces::empty()
        .with("property", Source::Object(property))
        .with("frame", Source::Object(frame))
}

fn unit(uuid: &str, title: &str) -> Ref {
    Ref::new(uuid, "resqml22.StratigraphicUnitInterpretation", title)
}

#[test]
fn statistics_per_unit_follow_the_frame_and_skip_unnamed_intervals() {
    let lutetian = unit("44444444-0000-4000-8000-000000000004", "Lutétien");
    let ypresian = unit("55555555-0000-4000-8000-000000000005", "Yprésien");
    let units = vec![
        Some(lutetian.clone()),
        None,
        Some(ypresian.clone()),
        Some(lutetian.clone()),
        Some(ypresian.clone()),
    ];
    let raw = vec![4, 100, -999, 8, 6];
    let pieces = well(raw.clone(), units.clone(), FRAME);
    let plan = run(&pieces, Choices::new().set("by", "unit")).expect("planned");
    let table = table(&plan);

    // The naive reading: the unit's rank in the frame's order, a null as NaN.
    let values: Vec<f64> = raw
        .iter()
        .map(|&v| if v == -999 { f64::NAN } else { v as f64 })
        .collect();
    let keys: Vec<Option<i64>> = units
        .iter()
        .map(|u| u.as_ref().map(|u| if u.uuid == lutetian.uuid { 1 } else { 2 }))
        .collect();
    let expected = naive(&values, &keys);
    assert_eq!(
        texts(&table, "unit"),
        vec!["Lutétien".to_string(), "Yprésien".to_string()]
    );
    assert_eq!(
        texts(&table, "unit uuid"),
        vec![lutetian.uuid.clone(), ypresian.uuid.clone()]
    );
    assert_eq!(
        integers(&table, "value count"),
        expected.iter().map(|row| row.1).collect::<Vec<_>>()
    );
    assert_eq!(
        integers(&table, "null count"),
        vec![0, 1],
        "the null of the Yprésien is counted there"
    );
    let means = reals(&table, "mean");
    assert!(
        close(means[0], expected[0].5) && close(means[1], expected[1].5),
        "{means:?}"
    );
    assert_eq!(means[1], 6.0, "the null -999 is never averaged");
    assert_eq!(
        reported(&plan, "ungroupedCount"),
        1.0,
        "the interval with no unit"
    );
    assert_eq!(
        column(&table, "mean").property_kind.title,
        "code",
        "the property's own kind"
    );
}

#[test]
fn the_units_of_one_frame_say_nothing_of_another() {
    let lutetian = unit("44444444-0000-4000-8000-000000000004", "Lutétien");
    let pieces = well(
        vec![1, 2],
        vec![Some(lutetian.clone()), Some(lutetian)],
        "99999999-0000-4000-8000-000000000009",
    );
    let refused = run(&pieces, Choices::new().set("by", "unit")).expect_err("another frame");
    assert_eq!(refused.refusals()[0].slot.as_deref(), Some("frame"), "{refused}");
}
