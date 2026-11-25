//! The limits on suites built by hand: classes, runs, ranks, the hole, and where a
//! limit sits among the nodes.
//!
//! The form is the specification's - a frame of `NodeCount` nodes has `NodeCount -
//! 1` intervals, each running from a node to the next (RESQML TRG 2.2 §7.1) - and
//! the numbers are the tests' own. Each test says, in its last lines, what to break
//! to see it fail.

use super::*;
use crate::frame_values::Categories;
use crate::property::IndexableElement;
use crate::table::{ColumnMeta, Lookup, Rows};
use crate::values::Values;
use crate::well_strata::{Section, Source, Unit};

fn nodes(md: &[f64]) -> Support {
    Support::Nodes { md: md.to_vec() }
}

fn intervals(md: &[f64]) -> Support {
    Support::Intervals {
        top: md[..md.len() - 1].to_vec(),
        base: md[1..].to_vec(),
    }
}

fn a() -> Option<Class> {
    Some(Class::Code(1))
}

fn b() -> Option<Class> {
    Some(Class::Code(2))
}

fn classes_on(support: Support, classes: Vec<Option<Class>>) -> Classification {
    Classification::new(support, classes).expect("one class per element")
}

/// The limits as `(kind, code, rank)`, for a suite of codes.
fn seen(transitions: &Transitions) -> Vec<(LimitKind, i64, usize)> {
    transitions
        .limits
        .iter()
        .map(|limit| {
            let Class::Code(code) = limit.class else {
                panic!("codes only")
            };
            (limit.kind, code, limit.rank)
        })
        .collect()
}

fn measures(support: Support, values: &[Option<f64>], continuous: bool) -> FrameValues {
    FrameValues {
        frame: "f1".to_string(),
        property: "p1".to_string(),
        title: "P".to_string(),
        element: if support.is_intervals() {
            IndexableElement::Intervals
        } else {
            IndexableElement::Nodes
        },
        support,
        values: values.iter().map(|value| value.map(Value::Number)).collect(),
        continuous,
        uom: None,
        kind: None,
        kind_uri: None,
        range: None,
        categories: None,
        time: None,
        realization: None,
        derivation: None,
        notes: Vec::new(),
    }
}

// -- the runs and their ranks ---------------------------------------------------------

/// A class that comes back is two runs of that class, never a refusal: the
/// pressure between 1 and 5, then above 6, then between 1 and 5 again gives
/// three runs, two boundaries and four limits, each with its rank.
///
/// Break it: in `transitions`, delete `*rank += 1;` (the second run is then the first).
#[test]
fn a_class_that_comes_back_is_a_second_run_with_its_own_rank() {
    let found = transitions(&classes_on(
        intervals(&[0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0]),
        vec![a(), a(), b(), b(), a(), a()],
    ));
    assert_eq!(
        found
            .runs
            .iter()
            .map(|run| (run.first, run.last, run.rank))
            .collect::<Vec<_>>(),
        [(0, 1, 1), (2, 3, 1), (4, 5, 2)]
    );
    assert_eq!(found.boundaries.len(), 2);
    assert_eq!(
        seen(&found),
        [
            (LimitKind::Base, 1, 1),
            (LimitKind::Top, 2, 1),
            (LimitKind::Base, 2, 1),
            (LimitKind::Top, 1, 2),
        ],
        "the first base of 1, the first top of 2, the first base of 2, the second top of 1"
    );
    assert_eq!(found.limits[3].run, 2, "the run the limit belongs to");
}

/// The top of the first run and the base of the last have no limit (nothing lies
/// beyond), and **no limit crosses a hole**: a run next to an element with no value
/// has no limit on that side, and the same class on both sides of a hole is two runs
/// with no boundary between them.
///
/// Break it: in `transitions`, delete the test `runs[above].last + 1 != runs[below].first`.
#[test]
fn no_limit_crosses_a_hole() {
    let found = transitions(&classes_on(
        nodes(&[0.0, 10.0, 20.0, 30.0, 40.0]),
        vec![a(), a(), None, b(), b()],
    ));
    assert_eq!(found.runs.len(), 2);
    assert!(
        found.boundaries.is_empty() && found.limits.is_empty(),
        "{:?}",
        found.limits
    );

    let around = transitions(&classes_on(nodes(&[0.0, 10.0, 20.0]), vec![a(), None, a()]));
    assert_eq!(
        around.runs.iter().map(|run| run.rank).collect::<Vec<_>>(),
        [1, 2],
        "one class either side of a hole is two runs"
    );
    assert!(around.limits.is_empty());

    let alone = transitions(&classes_on(nodes(&[0.0, 10.0]), vec![a(), a()]));
    assert!(
        alone.limits.is_empty(),
        "one run has no limit: nothing above, nothing below"
    );
}

// -- classifying ----------------------------------------------------------------------------

/// `value >= threshold` is at or above, `value < threshold` is below, an absent value
/// has no class, and the threshold is kept with its values.
///
/// Break it: in `of_threshold`, test `number > threshold`.
#[test]
fn a_value_on_the_threshold_is_at_or_above() {
    let values = measures(
        nodes(&[0.0, 10.0, 20.0, 30.0, 40.0, 50.0]),
        &[Some(1.0), Some(5.9), Some(6.0), Some(7.0), None, Some(2.0)],
        true,
    );
    let classified = Classification::of_threshold(&values, 6.0).expect("a continuous property");
    assert_eq!(
        classified.classes,
        [
            Some(Class::Below),
            Some(Class::Below),
            Some(Class::AtOrAbove),
            Some(Class::AtOrAbove),
            None,
            Some(Class::Below),
        ]
    );
    assert_eq!(
        classified.threshold.as_ref().map(|threshold| threshold.at),
        Some(6.0)
    );
}

/// Refused by name: a threshold on codes, a threshold that is not a number, a value
/// that is not a number, codes of a measure.
///
/// Break it: in `of_threshold`, delete the `!values.continuous` refusal.
#[test]
fn a_classification_refuses_by_name() {
    let codes = measures(nodes(&[0.0, 10.0]), &[Some(1.0), Some(2.0)], false);
    assert!(matches!(
        Classification::of_threshold(&codes, 1.5),
        Err(ClassifyRefusal::NotContinuous { .. })
    ));
    let measured = measures(nodes(&[0.0, 10.0]), &[Some(1.0), Some(2.0)], true);
    assert!(matches!(
        Classification::of_threshold(&measured, f64::NAN),
        Err(ClassifyRefusal::BadThreshold { .. })
    ));
    assert!(matches!(
        Classification::of_codes(&measured),
        Err(ClassifyRefusal::Continuous { .. })
    ));
    let mut texts = measures(nodes(&[0.0, 10.0]), &[Some(1.0), Some(2.0)], true);
    texts.values[1] = Some(Value::Text("x".to_string()));
    assert_eq!(
        Classification::of_threshold(&texts, 1.5),
        Err(ClassifyRefusal::NotNumbers { at: 1 })
    );
    assert_eq!(
        Classification::new(nodes(&[0.0, 10.0]), vec![a()]),
        Err(ClassifyRefusal::Length {
            classes: 1,
            elements: 2
        })
    );
}

/// The codes of a categorical property are the classes; a fraction is no code.
///
/// Break it: in `of_codes`, accept `Some(_) => None` for a fraction.
#[test]
fn the_codes_of_a_property_are_its_classes() {
    let mut values = measures(
        intervals(&[0.0, 10.0, 20.0, 30.0]),
        &[Some(3.0), None, Some(4.0)],
        false,
    );
    values.values[0] = Some(Value::Code(3));
    values.values[2] = Some(Value::Number(4.0));
    let classified = Classification::of_codes(&values).expect("codes");
    assert_eq!(
        classified.classes,
        [Some(Class::Code(3)), None, Some(Class::Code(4))]
    );
    values.values[2] = Some(Value::Number(4.5));
    assert_eq!(
        Classification::of_codes(&values),
        Err(ClassifyRefusal::NotACode { at: 2 })
    );
}

fn table() -> Categories {
    let columns: [(&str, bool, Values); 3] = [
        ("code", true, Values::Integer(vec![10, 20, 35])),
        ("density", false, Values::Floating(vec![2.5, 2.5, f64::NAN])),
        (
            "name",
            false,
            Values::Text(vec!["a".into(), "b".into(), "c".into()]),
        ),
    ];
    let lookup = Lookup {
        titles: columns.iter().map(|column| Some(column.0.to_string())).collect(),
        key_columns: vec![0],
        rows: Rows {
            columns: columns.iter().map(|column| Some(column.2.clone())).collect(),
            per_row: vec![1; 3],
            nulls: vec![None; 3],
            rows: 3,
            notes: Vec::new(),
        },
        notes: Vec::new(),
    };
    let meta = columns
        .iter()
        .map(|column| ColumnMeta {
            title: Some(column.0.to_string()),
            description: None,
            uom: None,
            kind: None,
            key: column.1,
            values_per_row: 1,
        })
        .collect();
    Categories::new("t".to_string(), "ColumnBasedTable".to_string(), lookup, meta)
}

/// A column of the table is a classification of its own: two codes whose rows hold the
/// same value are one class (one run), and an absent cell has no class. A code that
/// names no row, a column that is not there: refused.
///
/// Break it: in `of_column`, make the class `Class::Cell(row.to_string())` (the row, not its cell).
#[test]
fn a_column_of_the_table_classifies_by_the_cell_of_the_row() {
    let mut property = measures(intervals(&[0.0, 10.0, 20.0, 30.0, 40.0]), &[None; 4], false);
    property.values = vec![
        Some(Value::Code(20)),
        Some(Value::Code(10)),
        Some(Value::Code(35)),
        None,
    ];
    assert!(matches!(
        Classification::of_column(&property, 1),
        Err(ClassifyRefusal::NoTable { .. })
    ));
    property.categories = Some(table());

    let classified = Classification::of_column(&property, 1).expect("the density column");
    assert_eq!(
        classified.classes,
        [
            Some(Class::Cell("2.5".to_string())),
            Some(Class::Cell("2.5".to_string())),
            None,
            None
        ],
        "20 and 10 share a density, 35's is absent"
    );
    assert_eq!(transitions(&classified).runs.len(), 1, "one class, one run");

    assert_eq!(
        Classification::of_column(&property, 7),
        Err(ClassifyRefusal::NoSuchColumn {
            column: 7,
            columns: 3
        })
    );
    property.values[3] = Some(Value::Code(15));
    assert_eq!(
        Classification::of_column(&property, 2),
        Err(ClassifyRefusal::NoRow {
            at: 3,
            code: "15".to_string()
        })
    );
}

fn unit(uuid: &str) -> Unit {
    Unit {
        code: 0.0,
        uuid: Some(uuid.to_string()),
        uri: None,
        title: uuid.to_string(),
        rank: None,
        colour: None,
        colour_from: None,
        by_title: false,
    }
}

/// The units of a frame's stratigraphy are classes by identity; an interval that names
/// none has no class.
///
/// Break it: in `of_units`, class every interval `Class::Unit(String::new())`.
#[test]
fn the_units_of_a_frame_are_classes_by_identity() {
    let section = |top: f64, base: f64, unit: Option<usize>| Section {
        top,
        base,
        unit,
        several: Vec::new(),
    };
    let strata = Strata {
        frame: "f1".to_string(),
        title: "F1".to_string(),
        source: Source::IntervalUnits { ranks: Vec::new() },
        units: vec![unit("U1"), unit("U2")],
        sections: vec![
            section(0.0, 10.0, Some(0)),
            section(10.0, 20.0, Some(1)),
            section(20.0, 30.0, None),
        ],
        notes: Vec::new(),
    };
    let classified = Classification::of_units(&strata);
    assert_eq!(
        classified.classes,
        [
            Some(Class::Unit("u1".to_string())),
            Some(Class::Unit("u2".to_string())),
            None
        ]
    );
    assert_eq!(classified.support, intervals(&[0.0, 10.0, 20.0, 30.0]));
}

// -- placing ---------------------------------------------------------------------------------

fn limit_of(found: &Transitions, kind: LimitKind, code: i64) -> Limit {
    found
        .limits
        .iter()
        .find(|limit| limit.kind == kind && limit.class == Class::Code(code))
        .expect("that limit exists")
        .clone()
}

/// On intervals the limit is the node that separates them, whatever the anchor.
///
/// Break it: in `place`, answer `base[j]` for the intervals.
#[test]
fn on_intervals_the_limit_is_the_node_between_the_two() {
    let found = transitions(&classes_on(
        intervals(&[0.0, 10.0, 25.0, 30.0]),
        vec![a(), b(), b()],
    ));
    let base = limit_of(&found, LimitKind::Base, 1);
    for anchor in [Anchor::Inside, Anchor::Outside, Anchor::Interpolated] {
        let placed = found.place(&base, anchor).expect("the node between");
        assert_eq!((placed.md, placed.spot), (10.0, Spot::Node(1)));
    }
}

/// On nodes, **inside** puts the limit on the node of the run it bounds (the first
/// node of a top, the last of a base) and **outside** on the neighbour.
///
/// Break it: in `place`, swap the two `match limit.kind` arms of `Anchor::Inside`.
#[test]
fn on_nodes_the_anchor_says_inside_or_outside() {
    let found = transitions(&classes_on(
        nodes(&[0.0, 10.0, 20.0, 30.0]),
        vec![a(), a(), b(), b()],
    ));
    let base = limit_of(&found, LimitKind::Base, 1);
    let top = limit_of(&found, LimitKind::Top, 2);
    let md = |limit: &Limit, anchor: Anchor| found.place(limit, anchor).expect("on a node").md;
    assert_eq!(
        (md(&base, Anchor::Inside), md(&top, Anchor::Inside)),
        (10.0, 20.0)
    );
    assert_eq!(
        (md(&base, Anchor::Outside), md(&top, Anchor::Outside)),
        (20.0, 10.0)
    );
    assert_eq!(Anchor::default(), Anchor::Inside);
}

/// Interpolated: where the threshold is crossed, linearly in MD, on both limits of the
/// boundary; on a node when the value is the threshold; never over a gap wider than
/// the rule; refused for classes that are not a threshold.
///
/// Break it: in `place`, write `md[i] + (1.0 - t) * gap`.
#[test]
fn an_interpolated_limit_is_where_the_threshold_is_crossed() {
    let crossing = |md: &[f64], values: &[f64], threshold: f64| {
        let values = measures(
            nodes(md),
            &values.iter().map(|value| Some(*value)).collect::<Vec<_>>(),
            true,
        );
        transitions(&Classification::of_threshold(&values, threshold).expect("classified"))
    };

    // Rising: 3 at MD 10 and 9 at MD 20, threshold 5: a third of the way.
    let found = crossing(&[0.0, 10.0, 20.0, 30.0], &[1.0, 3.0, 9.0, 10.0], 5.0);
    assert_eq!(found.limits.len(), 2);
    for limit in &found.limits {
        let placed = found.place(limit, Anchor::Interpolated).expect("bridged");
        assert!((placed.md - (10.0 + 10.0 / 3.0)).abs() < 1e-9, "{}", placed.md);
        assert!(
            matches!(placed.spot, Spot::Between { lower: 1, upper: 2, t } if (t - 1.0 / 3.0).abs() < 1e-9)
        );
    }

    // Falling: 8 at MD 10 and 2 at MD 20, threshold 5: half way.
    let found = crossing(&[0.0, 10.0, 20.0, 30.0], &[9.0, 8.0, 2.0, 1.0], 5.0);
    assert_eq!(
        found
            .place(&found.limits[0], Anchor::Interpolated)
            .map(|placed| placed.md),
        Ok(15.0)
    );

    // On a node: the value of the node is the threshold (which is on the upper side).
    let found = crossing(&[0.0, 10.0, 20.0], &[1.0, 6.0, 9.0], 6.0);
    assert_eq!(
        found
            .place(&found.limits[0], Anchor::Interpolated)
            .map(|placed| (placed.md, placed.spot)),
        Ok((10.0, Spot::Node(1)))
    );
    let found = crossing(&[0.0, 10.0, 20.0], &[9.0, 6.0, 1.0], 6.0);
    assert_eq!(
        found
            .place(&found.limits[0], Anchor::Interpolated)
            .map(|placed| placed.spot),
        Ok(Spot::Node(1))
    );

    // Over a hole: nodes 3 and 4 are 97 apart, the rule bridges three times the median step.
    let found = crossing(&[0.0, 1.0, 2.0, 3.0, 100.0], &[1.0, 2.0, 3.0, 4.0, 10.0], 6.0);
    assert_eq!(
        found.place(&found.limits[0], Anchor::Interpolated),
        Err(PlaceRefusal::GapTooWide {
            lower: 3,
            upper: 4,
            gap: 97.0,
            max: 3.0
        })
    );
    // The two anchors that sit on nodes bridge nothing.
    assert!(found.place(&found.limits[0], Anchor::Inside).is_ok());

    let plain = transitions(&classes_on(nodes(&[0.0, 10.0]), vec![a(), b()]));
    assert_eq!(
        plain.place(&plain.limits[0], Anchor::Interpolated),
        Err(PlaceRefusal::NotAThreshold)
    );
}
