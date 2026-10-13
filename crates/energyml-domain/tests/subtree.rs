//! A unit's subtree — what a model made from it contains
//! (the earth-models design).
//!
//! Built from columns written by hand, one rule per test. What the chart of the
//! BRGM says about the same nesting is confronted in
//! `energyml-build/tests/earth_model_from_stratigraphy.rs`, against a source
//! that is not this code.

use energyml_domain::stratigraphy::{ColumnRank, ColumnUnit, Columns, Named, StratigraphicColumn, subtree, whole_column};

fn named(uuid: &str, title: &str) -> Named {
    Named { uuid: uuid.into(), uri: uuid.into(), title: title.into(), kind: "StratigraphicUnitInterpretation".into() }
}

fn unit(uuid: &str, age: Option<[f64; 2]>) -> ColumnUnit {
    ColumnUnit {
        unit: named(uuid, uuid),
        index: 0,
        role: None,
        resolved: true,
        represented: Vec::new(),
        age,
        models: Vec::new(),
    }
}

fn rank(uuid: &str, number: i64, units: Vec<ColumnUnit>) -> ColumnRank {
    let units = units.into_iter().enumerate().map(|(at, mut one)| {
        one.index = at as i64;
        one
    });
    ColumnRank {
        rank: Named { uuid: uuid.into(), uri: uuid.into(), title: uuid.into(), kind: "StratigraphicColumnRankInterpretation".into() },
        number: Some(number),
        criteria: "age".into(),
        units: units.collect(),
        contacts: Vec::new(),
        notes: Vec::new(),
    }
}

fn column(ranks: Vec<ColumnRank>) -> Columns {
    Columns {
        columns: vec![StratigraphicColumn {
            column: Named { uuid: "col".into(), uri: "col".into(), title: "column".into(), kind: "StratigraphicColumn".into() },
            ranks,
            models: Vec::new(),
        }],
        loose_ranks: Vec::new(),
        notes: Vec::new(),
    }
}

fn titles(units: &[Named]) -> Vec<&str> {
    units.iter().map(|one| one.uuid.as_str()).collect()
}

/// Series and stages of the Paleogene, dated as the chart dates them.
fn dated() -> Columns {
    column(vec![
        rank("series", 5, vec![
            unit("oligocene", Some([33.9, 23.03])),
            unit("eocene", Some([56.0, 33.9])),
            unit("paleocene", Some([66.0, 56.0])),
        ]),
        rank("stages", 7, vec![
            unit("rupelian", Some([33.9, 27.82])),
            unit("priabonian", Some([37.71, 33.9])),
            unit("bartonian", Some([41.03, 37.71])),
            unit("lutetian", Some([47.8, 41.03])),
            unit("ypresian", Some([56.0, 47.8])),
            unit("thanetian", Some([59.24, 56.0])),
        ]),
    ])
}

#[test]
fn by_age_a_unit_contains_the_finer_units_whose_span_is_inside_its_own() {
    let trees = subtree(&dated(), "eocene").expect("listed");
    assert_eq!(trees.len(), 1);
    let tree = &trees[0];
    assert_eq!(titles(&tree.ranks[0].units), ["eocene"]);
    assert!(!tree.ranks[0].whole, "the series rank holds three, the model one");
    // Shared limits (33.9 and 56.0) are inside, to a year: Rupelian and
    // Thanetian touch the Eocene and stay out.
    assert_eq!(titles(&tree.ranks[1].units), ["priabonian", "bartonian", "lutetian", "ypresian"]);
    assert_eq!(tree.ranks[1].number, Some(7), "a restricted rank keeps its number");
    assert!(tree.unplaced.is_empty(), "{:?}", tree.unplaced);
}

#[test]
fn a_unit_that_overlaps_a_limit_is_named_never_put_on_one_side() {
    let mut columns = dated();
    columns.columns[0].ranks[1].units[1].age = Some([38.0, 33.0]); // across the Eocene's top
    let tree = &subtree(&columns, "eocene").expect("listed")[0];
    assert_eq!(titles(&tree.ranks[1].units), ["bartonian", "lutetian", "ypresian"]);
    assert_eq!(tree.unplaced.len(), 1);
    assert_eq!(tree.unplaced[0].0.uuid, "priabonian");
    assert!(tree.unplaced[0].1.contains("overlaps"), "{}", tree.unplaced[0].1);
}

/// The same column with no age at all, and a unit shared between the ranks:
/// the Oligocene and the Paleocene are not subdivided at the stage rank.
fn undated() -> Columns {
    column(vec![
        rank("series", 5, vec![unit("oligocene", None), unit("eocene", None), unit("paleocene", None)]),
        rank("stages", 7, vec![
            unit("oligocene", None),
            unit("priabonian", None),
            unit("bartonian", None),
            unit("paleocene", None),
        ]),
    ])
}

#[test]
fn without_ages_the_units_between_two_shared_markers_belong_to_the_unit_between_them() {
    let tree = &subtree(&undated(), "eocene").expect("listed")[0];
    assert_eq!(titles(&tree.ranks[1].units), ["priabonian", "bartonian"]);
    assert!(tree.unplaced.is_empty());
}

#[test]
fn a_finer_rank_that_lists_the_unit_itself_does_not_subdivide_it() {
    let tree = &subtree(&undated(), "oligocene").expect("listed")[0];
    assert_eq!(titles(&tree.ranks[1].units), ["oligocene"]);
}

#[test]
fn a_rank_that_states_no_age_and_shares_nothing_is_left_out_by_its_name() {
    let columns = column(vec![
        rank("series", 5, vec![unit("oligocene", None), unit("eocene", None)]),
        rank("stages", 7, vec![unit("priabonian", None), unit("bartonian", None)]),
    ]);
    let tree = &subtree(&columns, "eocene").expect("listed")[0];
    assert_eq!(tree.ranks.len(), 1, "only the root's rank");
    assert_eq!(tree.unplaced.len(), 1);
    assert_eq!(tree.unplaced[0].0.uuid, "stages");
    assert!(tree.unplaced[0].1.contains("shares no unit"), "{}", tree.unplaced[0].1);
}

#[test]
fn markers_that_bound_two_units_do_not_say_which_owns_what_is_between() {
    // The Eocene and the Oligocene both lie between the markers `neogene` and
    // `paleocene`: the stages between them belong to one or the other, and the
    // file does not say which.
    let columns = column(vec![
        rank("series", 5, vec![
            unit("neogene", None),
            unit("oligocene", None),
            unit("eocene", None),
            unit("paleocene", None),
        ]),
        rank("stages", 7, vec![unit("neogene", None), unit("rupelian", None), unit("priabonian", None), unit("paleocene", None)]),
    ]);
    let tree = &subtree(&columns, "eocene").expect("listed")[0];
    assert_eq!(tree.ranks.len(), 1);
    assert!(tree.unplaced[0].1.contains("not alone"), "{}", tree.unplaced[0].1);
}

#[test]
fn a_unit_no_rank_lists_is_refused() {
    let refused = subtree(&dated(), "cretaceous").expect_err("nothing lists it");
    assert!(refused.contains("listed by no rank"), "{refused}");
}

#[test]
fn a_whole_column_is_every_rank_taken_as_it_is() {
    let columns = dated();
    let tree = whole_column(&columns.columns[0]);
    assert_eq!(tree.ranks.len(), 2);
    assert!(tree.ranks.iter().all(|one| one.whole));
    assert_eq!(tree.ranks[1].units.len(), 6);
}
