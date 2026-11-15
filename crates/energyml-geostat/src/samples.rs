//! What every point interpolator does with its samples before it computes
//! anything: set aside the ones with an undefined value or coordinate, measure
//! Euclidean distances, and refuse samples that sit on one another. One copy
//! for IDW, kriging, RBF, the empirical variogram and MBA, so a change to what
//! counts as usable is made once.

use crate::Error;

/// The samples that take part - a finite value at a finite position - and how
/// many were set aside.
pub(crate) fn usable<const N: usize>(samples: &[[f64; N]], values: &[f64]) -> (Vec<([f64; N], f64)>, usize) {
    let usable: Vec<([f64; N], f64)> = samples
        .iter()
        .zip(values)
        .filter(|(point, value)| value.is_finite() && point.iter().all(|c| c.is_finite()))
        .map(|(point, value)| (*point, *value))
        .collect();
    let set_aside = samples.len() - usable.len();
    (usable, set_aside)
}

/// The Euclidean distance between two positions.
pub(crate) fn distance<const N: usize>(a: &[f64; N], b: &[f64; N]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f64>()
        .sqrt()
}

/// Refuse two usable samples within `within` of each other, by their indices
/// among the usable ones. Coincident samples make two identical rows of a
/// matrix - singular by construction, and better refused with both named than
/// found as a mysterious zero pivot.
pub(crate) fn refuse_coincident<const N: usize>(
    usable: &[([f64; N], f64)],
    within: f64,
) -> Result<(), Error> {
    for (at, (a, _)) in usable.iter().enumerate() {
        for (offset, (b, _)) in usable[at + 1..].iter().enumerate() {
            if distance(a, b) <= within {
                return Err(Error::DuplicateSamplePositions {
                    first: at,
                    second: at + 1 + offset,
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sample with an undefined value or coordinate is set aside and counted;
    /// the others keep their order.
    #[test]
    fn what_is_undefined_is_set_aside_and_counted() {
        let samples = [[0.0, 0.0], [1.0, f64::NAN], [2.0, 2.0], [3.0, 3.0]];
        let values = [1.0, 2.0, f64::NAN, 4.0];
        let (usable, set_aside) = usable(&samples, &values);
        assert_eq!(usable, vec![([0.0, 0.0], 1.0), ([3.0, 3.0], 4.0)]);
        assert_eq!(set_aside, 2);
        // Break it: drop the `is_finite` test on the coordinates and the second
        // sample is kept.
    }

    /// Two samples within the tolerance are refused by their indices among the
    /// usable ones.
    #[test]
    fn coincident_samples_are_refused_by_index() {
        let usable = [([0.0, 0.0], 1.0), ([5.0, 0.0], 2.0), ([5.0, 1e-9], 3.0)];
        assert_eq!(
            refuse_coincident(&usable, 1e-6),
            Err(Error::DuplicateSamplePositions { first: 1, second: 2 })
        );
        assert_eq!(refuse_coincident(&usable[..2], 1e-6), Ok(()));
        assert_eq!(distance(&[0.0, 0.0], &[3.0, 4.0]), 5.0);
        // Break it: compare `>=` instead of `<=` and the duplicate passes.
    }
}
