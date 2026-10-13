//! Values split by a key — per zone, per stratigraphic unit.
//!
//! `energyml-domain` already has the statistics; what is missing is the
//! splitting. This is the splitting and nothing more. The statistics of each group are
//! `energyml_domain::stats::Summary`, computed by the caller: two
//! implementations of a mean would be two answers to the same question.
//!
//! # What can be wrong, and is held here
//!
//! - **A key belongs to the element at the same position**, so the two lists
//!   must be the same length; a shorter one would pair every value after the
//!   first gap with its neighbour's zone and still produce a table. Refused,
//!   with both counts.
//! - **An element with no key is in no group** — a null zone code, an interval
//!   the frame leaves without a unit — and is counted, never folded into a
//!   group of its own that would read as a zone.
//! - **A null value stays in its group, as a `NaN`**: the summary counts it
//!   apart and never averages it, and the count is part of the answer — a zone
//!   with ten values and nine nulls is not a zone with one value.
//! - **Groups come in key order**, so the same data makes the same table.

use std::collections::BTreeMap;

use crate::Error;

/// One group: its key, and its values in element order, nulls included.
#[derive(Debug, Clone, PartialEq)]
pub struct Group<K> {
    /// What the elements share.
    pub key: K,
    /// The values of its elements, `NaN` for a null.
    pub values: Vec<f64>,
}

/// Values split by key.
#[derive(Debug, Clone, PartialEq)]
pub struct Grouped<K> {
    /// The groups, in key order.
    pub groups: Vec<Group<K>>,
    /// Elements with no key — in no group.
    pub ungrouped: usize,
}

/// Split `values` by the key of the element at the same position.
pub fn partition<K: Ord + Clone>(values: &[f64], keys: &[Option<K>]) -> Result<Grouped<K>, Error> {
    if values.len() != keys.len() {
        return Err(Error::KeysAndValues { keys: keys.len(), values: values.len() });
    }
    let mut groups: BTreeMap<K, Vec<f64>> = BTreeMap::new();
    let mut ungrouped = 0usize;
    for (value, key) in values.iter().zip(keys) {
        match key {
            Some(key) => groups.entry(key.clone()).or_default().push(*value),
            None => ungrouped += 1,
        }
    }
    Ok(Grouped { groups: groups.into_iter().map(|(key, values)| Group { key, values }).collect(), ungrouped })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_split_values_and_nulls_stay_in_their_group() {
        let values = [1.0, 2.0, f64::NAN, 4.0, 5.0];
        let keys = [Some(7), Some(3), Some(7), None, Some(7)];
        let found = partition(&values, &keys).unwrap();
        assert_eq!(found.ungrouped, 1);
        assert_eq!(found.groups.iter().map(|g| g.key).collect::<Vec<_>>(), vec![3, 7]);
        assert_eq!(found.groups[0].values, vec![2.0]);
        let seven = &found.groups[1].values;
        assert_eq!((seven[0], seven[2], seven.len()), (1.0, 5.0, 3));
        assert!(seven[1].is_nan());
    }

    #[test]
    fn keys_and_values_of_two_lengths_are_refused() {
        assert_eq!(partition(&[1.0, 2.0], &[Some(1)]).unwrap_err(), Error::KeysAndValues { keys: 1, values: 2 });
    }
}
