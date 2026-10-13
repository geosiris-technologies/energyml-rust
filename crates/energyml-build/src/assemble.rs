//! From a read collection to the whole set of objects a package will hold.
//!
//! Everything that can be *wrong* about an import lives here — which features
//! fall in which group, which places are the same place, which row of the table
//! a point set is — and none of it needs a store. The module that opens a
//! package and writes an `.h5` comes after and has no decisions left to take.
//!
//! # Deduplication, and the number that must survive it
//!
//! A SIGEOL export writes one feature per borehole **pass**, not per place: the
//! Bartonien file holds 10 374 features over 1 291 distinct positions, up to 88
//! of them on one point with identical properties. Importing them as they stand
//! would put the same coordinates in a package eighty-eight times.
//!
//! So places are made distinct — a deliberate choice — and **the count
//! before deduplication is carried out with them**, into the table's `passes`
//! column and into the activity. A package showing only the second number looks
//! like a far thinner survey than the ground actually holds, and nothing in it
//! would say otherwise.
//!
//! # Distinct on what, exactly
//!
//! On the three ordinates, compared **bit for bit**, not on a rounded key and
//! not with a tolerance. Two reasons, and the second is the one that decides:
//!
//! - a tolerance is a decision about how far apart two boreholes have to be to
//!   be two boreholes, and nothing in this file supports one. The SIGEOL
//!   coordinates are already rounded to four decimal degrees by the producer —
//!   about 7 m — so the grouping they imply is the producer's, not ours;
//! - `NaN != NaN`, and a coordinate that is not a number is a coordinate this
//!   import has no business quietly merging. Comparing the bits makes two `NaN`s
//!   equal, which is what `hdf5_write` does for the same reason.
//!
//! Order is kept: first seen, first written. A package whose points move between
//! two runs of the same file is a package nobody can diff.

use std::collections::{BTreeMap, HashSet};

use crate::geojson::{Collection, Position};
use crate::sigeol::{self, Side, Unit};

/// One group: a unit, one of its surfaces, and the places boreholes met it.
#[derive(Debug, Clone)]
pub struct Group {
    /// The unit.
    pub unit: Unit,
    /// Top or base.
    pub side: Side,
    /// The stage the surface lies in.
    pub stage: String,
    /// Distinct places, in the order they were first seen.
    pub points: Vec<Position>,
    /// How many source features fed this group, before deduplication.
    pub features: usize,
}

impl Group {
    /// How many places the deduplication removed.
    pub fn duplicates(&self) -> usize {
        self.features.saturating_sub(self.points.len())
    }
}

/// What a collection came to, and what it did not.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// The groups, ordered by unit then side, so two runs write the same package.
    pub groups: Vec<Group>,
    /// Features that carried no unit this module recognises.
    pub not_units: usize,
    /// Features whose unit does not touch the queried stage.
    pub untouched: usize,
    /// Features whose age bounds do not order, so no side could be decided.
    pub unordered: usize,
    /// Features carrying no position at all.
    pub unplaced: usize,
    /// Features that are **the same feature** as one already read — same `id`,
    /// same places — which is what a unit stated in two exports is: the
    /// Lutétien unit whose two ages are both the Lutétien is in the
    /// `olderNamedAge` file *and* in the `youngerNamedAge` file, one pass of one
    /// borehole twice (measured on a real pair of exports: all 8 311 features of
    /// the first are in the second). A pass is counted once.
    pub repeated: usize,
}

impl Plan {
    /// Every feature the plan did **not** take, with its reason.
    ///
    /// A count on its own would be the thing this workspace refuses everywhere:
    /// *17 features skipped* is indistinguishable from a file that had 17 fewer.
    pub fn skipped(&self) -> Vec<String> {
        let mut out = Vec::new();
        let say = |n: usize, why: &str| (n > 0).then(|| format!("{n} {why}"));
        out.extend(say(self.not_units, "feature(s) carry no GeoSciML unit this reader knows"));
        out.extend(say(
            self.untouched,
            "feature(s) name a unit that does not touch the queried stage",
        ));
        out.extend(say(
            self.unordered,
            "feature(s) have no numeric age bounds, so neither end could be called the young one",
        ));
        out.extend(say(self.unplaced, "feature(s) carry no position"));
        out.extend(say(
            self.repeated,
            "feature(s) are one already read from another file (same id, same places): a pass stated twice counts once",
        ));
        out
    }

    /// Positions kept, over every group.
    pub fn points(&self) -> usize {
        self.groups.iter().map(|g| g.points.len()).sum()
    }

    /// Source features taken, over every group.
    pub fn features(&self) -> usize {
        self.groups.iter().map(|g| g.features).sum()
    }
}

/// Group a collection's features against one queried stage.
///
/// `stage` is what decides top from bottom, and it is **not in the file** — see
/// [`crate::sigeol`]. A caller with none has no sides to give.
pub fn plan(collection: &Collection, stage: &str) -> Plan {
    // Keyed and ordered, so the groups come out the same way twice. A `HashMap`
    // here would write a package whose object order changed between runs, which
    // is a diff nobody can read.
    let mut groups: BTreeMap<(String, Side), Group> = BTreeMap::new();
    let mut plan = Plan::default();
    // What was read, by `id` **and** places: an `id` alone is a producer's own
    // key and two layers may reuse one, so it only counts together with the
    // same positions, compared as bits like everywhere else here. A feature
    // without an `id` is never taken for a repeat.
    let mut read: HashSet<(String, Vec<[u64; 4]>)> = HashSet::new();

    for feature in &collection.features {
        if let Some(id) = &feature.id {
            let places = feature
                .positions()
                .map(|at| [at.lon.to_bits(), at.lat.to_bits(), at.z.map_or(0, f64::to_bits), u64::from(at.z.is_some())])
                .collect();
            if !read.insert((id.clone(), places)) {
                plan.repeated += 1;
                continue;
            }
        }
        let Some(unit) = sigeol::unit_of(&feature.properties) else {
            plan.not_units += 1;
            continue;
        };
        if !unit.ordered {
            plan.unordered += 1;
            continue;
        }
        let Some(side) = unit.side_for(stage) else {
            plan.untouched += 1;
            continue;
        };
        let mut positions = feature.positions().peekable();
        if positions.peek().is_none() {
            plan.unplaced += 1;
            continue;
        }
        let key = unit.key(side);
        let entry = groups.entry(key).or_insert_with(|| Group {
            unit: unit.clone(),
            side,
            stage: stage.to_string(),
            points: Vec::new(),
            features: 0,
        });
        // One feature is one borehole pass, whatever it carries: the count is of
        // *features*, and the places it adds may be none new at all.
        entry.features += 1;
        for position in positions {
            if !entry.points.iter().any(|kept| same_place(kept, &position)) {
                entry.points.push(position);
            }
        }
    }

    plan.groups = groups.into_values().collect();
    plan
}

/// Two positions are the same place when their three ordinates are the same
/// bits.
///
/// Bits and not `==`: `NaN != NaN`, and a coordinate that is not a number is not
/// one this import should quietly merge away. `hdf5_write` compares the same way
/// and for the same reason.
fn same_place(a: &Position, b: &Position) -> bool {
    let bits = |v: f64| v.to_bits();
    let z = |p: &Position| p.z.map(bits);
    bits(a.lon) == bits(b.lon) && bits(a.lat) == bits(b.lat) && z(a) == z(b)
}

/// The rows of the table, one per group, in the groups' own order.
///
/// Keys start at **1** rather than 0. Not decoration: the key column exists so a
/// property is matched against a key rather than counted from the top, and a
/// table keyed `0..n-1` makes the two indistinguishable — which is exactly how
/// a real table once mislabelled every facies.
pub fn rows(plan: &Plan) -> Vec<crate::table::Row> {
    plan.groups
        .iter()
        .enumerate()
        .map(|(at, group)| crate::table::Row {
            key: at as i64 + 1,
            unit: group.unit.name.clone().unwrap_or_else(|| "?".to_string()),
            referential: group.unit.href.clone().unwrap_or_default(),
            stage: group.stage.clone(),
            side: group.side.label().to_string(),
            // Written even when absent — a group only exists when its bounds
            // ordered, so these are always there; `0.0` would be a value nobody
            // wrote, and the shape says so if that ever changes.
            young_ma: group.unit.young.ma.unwrap_or_default(),
            old_ma: group.unit.old.ma.unwrap_or_default(),
            features: group.features as i64,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geojson;

    fn feature(unit: &str, lon: f64, lat: f64, z: f64, older: (&str, f64), younger: (&str, f64)) -> String {
        format!(
            r#"{{"type":"Feature","geometry":{{"type":"Point","coordinates":[{lon},{lat},{z}]}},
                "properties":{{
                  "composition":[{{"material":{{"StratiRockMaterial":{{"lithostratigraphy":{{
                     "@href":"https://referentiel-lithostrat.brgm-rec.fr/#/lithostrat/search/details/{unit}",
                     "@title":"{unit}"}}}}}}}}],
                  "geologicHistory":[{{
                     "olderNamedAge":{{"@title":"{}"}},
                     "youngerNamedAge":{{"@title":"{}"}},
                     "numericAge":{{"NumericAgeRange":{{
                        "olderBoundDate":{{"Quantity":{{"value":{}}}}},
                        "youngerBoundDate":{{"Quantity":{{"value":{}}}}}}}}}}}]}}}}"#,
            older.0, younger.0, older.1, younger.1
        )
    }

    fn collection(features: &[String]) -> Collection {
        let text = format!(
            r#"{{"type":"FeatureCollection","features":[{}]}}"#,
            features.join(",")
        );
        let read = geojson::read(&text).expect("a collection");
        assert!(read.refused.is_empty(), "{:?}", read.refused);
        read
    }

    #[test]
    fn eighty_eight_passes_on_one_place_are_one_point_and_the_count_survives() {
        // The shape of the real files: one feature per borehole pass. The place
        // is written once; the number of passes is what says the survey is not
        // as thin as the point count suggests.
        let one = feature("u", 2.29, 48.87, 58.0, ("Bartonien", 37.8), ("Lutétien", 47.8));
        let made = plan(&collection(&[one.clone(), one.clone(), one]), "Bartonien");
        assert_eq!(made.groups.len(), 1);
        assert_eq!(made.groups[0].points.len(), 1);
        assert_eq!(made.groups[0].features, 3);
        assert_eq!(made.groups[0].duplicates(), 2);
    }

    #[test]
    fn two_places_differing_only_in_elevation_are_two_places() {
        // Two boreholes rounded to the same degree pair, at different ground
        // levels. Merging them on the horizontal alone would lose one silently.
        let a = feature("u", 2.29, 48.87, 58.0, ("Bartonien", 37.8), ("Lutétien", 47.8));
        let b = feature("u", 2.29, 48.87, 43.9, ("Bartonien", 37.8), ("Lutétien", 47.8));
        let made = plan(&collection(&[a, b]), "Bartonien");
        assert_eq!(made.groups[0].points.len(), 2);
    }

    #[test]
    fn a_unit_and_a_side_make_a_group_and_the_order_does_not_move() {
        // Two units, one stage. Ordered by key so two runs of one file write the
        // same package — a package whose object order changed between runs is a
        // diff nobody can read.
        let a = feature("aaa", 2.29, 48.87, 58.0, ("Bartonien", 37.8), ("Lutétien", 47.8));
        let b = feature("bbb", 2.40, 48.90, 50.0, ("Bartonien", 37.8), ("Bartonien", 41.2));
        let first = plan(&collection(&[a.clone(), b.clone()]), "Bartonien");
        let second = plan(&collection(&[b, a]), "Bartonien");
        assert_eq!(first.groups.len(), 2);
        let names = |p: &Plan| p.groups.iter().map(|g| g.unit.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&first), names(&second));
    }

    #[test]
    fn what_is_not_taken_is_counted_and_named() {
        // A count on its own is what this workspace refuses everywhere: *17
        // skipped* is indistinguishable from a file that had 17 fewer.
        let good = feature("u", 2.29, 48.87, 58.0, ("Bartonien", 37.8), ("Lutétien", 47.8));
        // A unit that does not touch the queried stage.
        let elsewhere = feature("v", 2.30, 48.88, 55.0, ("Yprésien", 47.8), ("Yprésien", 56.0));
        // A feature that is not a GeologicUnit at all.
        let other = r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[2.3,48.9]},
                        "properties":{"title":"un polygone"}}"#
            .to_string();
        let made = plan(&collection(&[good, elsewhere, other]), "Bartonien");
        assert_eq!(made.groups.len(), 1);
        assert_eq!(made.untouched, 1);
        assert_eq!(made.not_units, 1);
        let said = made.skipped();
        assert_eq!(said.len(), 2);
        assert!(said.iter().any(|s| s.contains("does not touch")), "{said:?}");
        assert!(said.iter().any(|s| s.contains("no GeoSciML unit")), "{said:?}");
    }

    #[test]
    fn a_file_with_nothing_to_take_says_so_rather_than_producing_an_empty_package() {
        let elsewhere = feature("v", 2.30, 48.88, 55.0, ("Yprésien", 47.8), ("Yprésien", 56.0));
        let made = plan(&collection(&[elsewhere]), "Bartonien");
        assert!(made.groups.is_empty());
        assert_eq!(made.points(), 0);
        assert!(!made.skipped().is_empty());
    }

    #[test]
    fn the_rows_are_keyed_from_one_and_carry_the_pass_count() {
        let a = feature("aaa", 2.29, 48.87, 58.0, ("Bartonien", 37.8), ("Lutétien", 47.8));
        let b = feature("bbb", 2.40, 48.90, 50.0, ("Bartonien", 37.8), ("Bartonien", 41.2));
        let made = plan(&collection(&[a.clone(), a, b]), "Bartonien");
        let rows = rows(&made);
        assert_eq!(rows.len(), 2);
        // From 1: a table keyed 0..n-1 makes key and position indistinguishable,
        // which is how a real table once mislabelled every facies.
        assert_eq!(rows[0].key, 1);
        assert_eq!(rows[1].key, 2);
        assert_eq!(rows[0].side, "Top");
        // Two passes on one place, and the row says both numbers.
        let doubled = rows.iter().find(|r| r.features == 2).expect("a row of two passes");
        assert_eq!(doubled.stage, "Bartonien");
        assert!(doubled.old_ma > doubled.young_ma);
    }
}
