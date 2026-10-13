//! Inverse-distance weighting - the first interpolator of the crate, and
//! deliberately the plain one (trivial to write, testable against analytic cases).
//!
//! Shepard's method: the value at a query point is the weighted mean of
//! the sample values, each weight `1 / d^p`. Three decisions worth
//! stating, because each has a wrong alternative that looks fine:
//!
//! - **A query sitting on a sample returns that sample's value exactly.**
//!   `1 / 0` is not a weight; the standard rule is that IDW is an *exact*
//!   interpolator, and returning the sample beats returning `inf/inf`.
//!   "On" means within [`Idw::exact_within`] - bitwise equality would make
//!   exactness depend on how the caller computed its coordinates.
//! - **A `NaN` sample is set aside, never averaged** (the house rule) -
//!   one undefined corner must not erase a whole neighbourhood. How many
//!   were set aside is in the answer.
//! - **A query with no sample in reach yields `NaN`, not a refusal.** With
//!   a search radius, "nothing near here" is a legitimate per-point
//!   answer - the caller asked *where is the field defined*. Having no
//!   usable samples at all, by contrast, refuses up front: every query
//!   would be `NaN` and the call was malformed, which is different news.
//!
//! The neighbour search is linear per query - O(samples × queries). That
//! is the honest cost for the corpus sizes this serves (hundreds of wells,
//! thousands of grid nodes); a spatial index is worth adding when a
//! measurement says so, not before.
//!
//! TODO(reference): verify - Shepard's method is named without a full citation in this code; add the original reference (authors, year, title, venue, DOI) once checked.

use crate::Error;
use crate::barrier::{Barriers, keep_visible};

/// The parameters of an inverse-distance interpolation, dimension `N`.
///
/// The type is the builder: `Idw::new(2.0)` then `with_radius` /
/// `with_neighbours` as needed. `N` is the coordinate dimension - 2 for a
/// map-view surface, 3 for a volume - fixed at compile time because mixing
/// dimensions in one call is a bug, not a feature.
#[derive(Debug, Clone)]
pub struct Idw {
    /// The exponent `p` of `1 / d^p`. 2 is the usual choice; larger makes
    /// the field more local, 0 < p ≤ 1 makes it flatter than the data.
    pub power: f64,
    /// Only samples within this distance take part. `None` = all of them.
    pub radius: Option<f64>,
    /// Only the `k` nearest samples take part. `None` = all of them.
    /// Combined with `radius`, both must hold.
    pub neighbours: Option<usize>,
    /// A query closer than this to a sample *is* that sample. Defaults to
    /// 1e-12 in the units of the coordinates.
    pub exact_within: f64,
    /// Faults the interpolation must not see across - a sample whose segment
    /// to the query crosses one takes no part. `None` = no fault. Plan view
    /// only: the first two coordinates of a sample are what a trace cuts.
    pub barriers: Option<Barriers>,
}

impl Idw {
    /// Shepard's method with exponent `power`, no radius, all samples.
    pub fn new(power: f64) -> Self {
        Idw {
            power,
            radius: None,
            neighbours: None,
            exact_within: 1e-12,
            barriers: None,
        }
    }

    /// Restrict to samples within `radius` of the query.
    #[must_use]
    pub fn with_radius(mut self, radius: f64) -> Self {
        self.radius = Some(radius);
        self
    }

    /// Restrict to the `k` nearest samples of the query.
    #[must_use]
    pub fn with_neighbours(mut self, k: usize) -> Self {
        self.neighbours = Some(k);
        self
    }

    /// Faults as barriers: a sample the query cannot see takes no part.
    #[must_use]
    pub fn with_barriers(mut self, barriers: Barriers) -> Self {
        self.barriers = Some(barriers);
        self
    }

    /// Interpolate `values` known at `samples` onto `queries`.
    ///
    /// One value per query, in order; a query out of reach of every sample
    /// (radius, or no usable sample after the NaN set-aside) yields `NaN`.
    /// Refuses by name when the *call* is wrong: lengths that disagree, a
    /// power or radius that is not a positive finite number, `k = 0`, or
    /// no usable sample anywhere.
    pub fn interpolate<const N: usize>(
        &self,
        samples: &[[f64; N]],
        values: &[f64],
        queries: &[[f64; N]],
    ) -> Result<IdwAnswer, Error> {
        if samples.len() != values.len() {
            return Err(Error::MismatchedLengths {
                count: samples.len(),
                values: values.len(),
            });
        }
        if !(self.power.is_finite() && self.power > 0.0) {
            return Err(Error::BadPower { given: self.power });
        }
        if let Some(radius) = self.radius
            && !(radius.is_finite() && radius > 0.0)
        {
            return Err(Error::BadRadius { given: radius });
        }
        if self.neighbours == Some(0) {
            return Err(Error::NoNeighbours);
        }

        // The NaN set-aside: a sample with an undefined value or an
        // undefined coordinate takes no part, and is counted.
        let usable: Vec<(usize, [f64; N], f64)> = samples
            .iter()
            .zip(values)
            .enumerate()
            .filter(|(_, (point, value))| value.is_finite() && point.iter().all(|c| c.is_finite()))
            .map(|(at, (point, value))| (at, *point, *value))
            .collect();
        let set_aside = samples.len() - usable.len();
        if usable.is_empty() {
            return Err(Error::NoSample {
                given: samples.len(),
                set_aside,
            });
        }

        let radius2 = self.radius.map(|r| r * r);
        let exact2 = self.exact_within * self.exact_within;
        let mut out = Vec::with_capacity(queries.len());
        let mut unreached = 0usize;
        // Weights on distance, not distance²: d^p = (d²)^(p/2). The default
        // power is 2, whose weight is 1/d² - a division rather than a `powf`,
        // and the speed report measured the difference at seven times the
        // whole interpolation.
        let half = -self.power / 2.0;
        let weight = |d2: f64| if half == -1.0 { 1.0 / d2 } else { d2.powf(half) };
        // Reused across queries; (distance², sample) of the samples in reach.
        let mut near: Vec<(f64, usize)> = Vec::with_capacity(usable.len());
        for query in queries {
            near.clear();
            let mut exact: Option<f64> = None;
            for (at, (_, point, value)) in usable.iter().enumerate() {
                let d2: f64 = point.iter().zip(query).map(|(a, b)| (a - b) * (a - b)).sum();
                if d2 <= exact2 {
                    exact = Some(*value);
                    break;
                }
                if radius2.is_none_or(|r2| d2 <= r2) {
                    near.push((d2, at));
                }
            }
            if let Some(value) = exact {
                out.push(value);
                continue;
            }
            match &self.barriers {
                // Behind a fault: not a neighbour, whatever its distance -
                // tested after the radius, which is cheap, and before the cap,
                // which is the rule. The test is on the first two coordinates
                // - a fault is a trace in plan view - so a 3D call is cut by
                // its map-view shadow.
                Some(faults) => keep_visible(&mut near, self.neighbours, |at| {
                    faults.blocks(plan(&usable[at].1), plan(query))
                }),
                None => {
                    if let Some(k) = self.neighbours
                        && near.len() > k
                    {
                        // The k nearest: a partial sort is all that is needed.
                        near.select_nth_unstable_by(k - 1, |a, b| a.0.total_cmp(&b.0));
                        near.truncate(k);
                    }
                }
            }
            if near.is_empty() {
                out.push(f64::NAN);
                unreached += 1;
                continue;
            }
            let mut weighted = 0.0;
            let mut total = 0.0;
            for &(d2, at) in &near {
                let w = weight(d2);
                weighted += w * usable[at].2;
                total += w;
            }
            out.push(weighted / total);
        }
        Ok(IdwAnswer {
            values: out,
            set_aside,
            unreached,
        })
    }
}

/// The plan-view position of a sample: what a fault trace cuts.
pub(crate) fn plan<const N: usize>(point: &[f64; N]) -> [f64; 2] {
    [
        point.first().copied().unwrap_or(f64::NAN),
        point.get(1).copied().unwrap_or(f64::NAN),
    ]
}

/// What an interpolation answers - the values, and the two counts that
/// keep the answer honest.
#[derive(Debug, Clone, PartialEq)]
pub struct IdwAnswer {
    /// One value per query, in the queries' order. `NaN` = out of reach.
    pub values: Vec<f64>,
    /// Samples set aside for a `NaN` value or coordinate - never averaged.
    pub set_aside: usize,
    /// Queries no sample could reach (radius) - each is a `NaN` above.
    pub unreached: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_constant_field_is_reproduced_exactly() {
        // Whatever the weights do, a weighted mean of the same number is
        // that number - the first analytic case worth pinning.
        let samples = [[0.0, 0.0], [10.0, 0.0], [3.0, 7.0]];
        let values = [4.5, 4.5, 4.5];
        let queries = [[1.0, 1.0], [-5.0, 2.0], [100.0, 100.0]];
        let answer = Idw::new(2.0).interpolate(&samples, &values, &queries).unwrap();
        for value in &answer.values {
            assert!((value - 4.5).abs() < 1e-12);
        }
    }

    #[test]
    fn the_interpolator_is_exact_at_its_samples() {
        let samples = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let values = [1.0, 2.0, 3.0];
        let answer = Idw::new(2.0).interpolate(&samples, &values, &samples).unwrap();
        assert_eq!(answer.values, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn the_answer_stays_inside_the_data_bounds() {
        // A weighted mean with positive weights cannot leave [min, max] -
        // an implementation that can is not IDW.
        let samples = [[0.0, 0.0], [4.0, 0.0], [2.0, 3.0], [1.0, 1.0]];
        let values = [0.0, 10.0, 5.0, 2.0];
        let queries: Vec<[f64; 2]> = (0..50)
            .map(|at| [(at as f64) / 10.0, ((at * 7) % 30) as f64 / 10.0])
            .collect();
        let answer = Idw::new(2.0).interpolate(&samples, &values, &queries).unwrap();
        for value in &answer.values {
            assert!((0.0..=10.0).contains(value), "{value} left [0, 10]");
        }
    }

    #[test]
    fn the_midpoint_of_two_samples_is_their_mean_whatever_the_power() {
        for power in [0.5, 1.0, 2.0, 4.0] {
            let answer = Idw::new(power)
                .interpolate(&[[0.0, 0.0], [2.0, 0.0]], &[1.0, 3.0], &[[1.0, 0.0]])
                .unwrap();
            assert!((answer.values[0] - 2.0).abs() < 1e-12, "power {power}");
        }
    }

    #[test]
    fn a_higher_power_pulls_harder_towards_the_nearest_sample() {
        let samples = [[0.0, 0.0], [3.0, 0.0]];
        let values = [0.0, 1.0];
        let query = [[1.0, 0.0]]; // twice as close to the first sample
        let soft = Idw::new(1.0)
            .interpolate(&samples, &values, &query)
            .unwrap()
            .values[0];
        let hard = Idw::new(4.0)
            .interpolate(&samples, &values, &query)
            .unwrap()
            .values[0];
        assert!(
            hard < soft,
            "power 4 ({hard}) should sit closer to 0.0 than power 1 ({soft})"
        );
    }

    #[test]
    fn a_nan_sample_is_set_aside_and_counted_never_averaged() {
        let samples = [[0.0, 0.0], [2.0, 0.0], [1.0, 5.0]];
        let values = [1.0, 3.0, f64::NAN];
        let answer = Idw::new(2.0)
            .interpolate(&samples, &values, &[[1.0, 0.0]])
            .unwrap();
        assert_eq!(answer.set_aside, 1);
        assert!(
            (answer.values[0] - 2.0).abs() < 1e-12,
            "the NaN must not poison the mean"
        );
    }

    #[test]
    fn a_nan_coordinate_sets_its_sample_aside_too() {
        let samples = [[0.0, 0.0], [f64::NAN, 0.0]];
        let answer = Idw::new(2.0)
            .interpolate(&samples, &[1.0, 9.0], &[[5.0, 0.0]])
            .unwrap();
        assert_eq!(answer.set_aside, 1);
        assert!((answer.values[0] - 1.0).abs() < 1e-12);
    }

    /// **A fault is a barrier, whatever the distance.** The query sits a
    /// hair from the far sample, and still hears only the near one.
    #[test]
    fn a_sample_behind_a_fault_takes_no_part() {
        let fault = Barriers::from_polylines(&[vec![[5.0, -10.0], [5.0, 10.0]]]).unwrap();
        let samples = [[0.0, 0.0], [6.0, 0.0]];
        let values = [1.0, 100.0];
        let query = [[4.9, 0.0]];
        let seen = Idw::new(2.0)
            .interpolate(&samples, &values, &query)
            .unwrap()
            .values[0];
        let cut = Idw::new(2.0)
            .with_barriers(fault)
            .interpolate(&samples, &values, &query)
            .unwrap()
            .values[0];
        assert!(seen > 50.0, "without the fault the near sample dominates: {seen}");
        assert!(
            (cut - 1.0).abs() < 1e-12,
            "with the fault only the visible sample answers: {cut}"
        );
    }

    /// Every sample behind a fault: out of reach, said as such - `NaN` and
    /// counted, exactly like a radius nobody reaches.
    #[test]
    fn a_query_walled_off_from_every_sample_is_unreached() {
        let fault = Barriers::from_polylines(&[vec![[5.0, -10.0], [5.0, 10.0]]]).unwrap();
        let answer = Idw::new(2.0)
            .with_barriers(fault)
            .interpolate(&[[0.0, 0.0], [1.0, 1.0]], &[1.0, 2.0], &[[9.0, 0.0]])
            .unwrap();
        assert!(answer.values[0].is_nan());
        assert_eq!(answer.unreached, 1);
    }

    #[test]
    fn out_of_radius_is_nan_and_counted_not_a_refusal() {
        let answer = Idw::new(2.0)
            .with_radius(1.0)
            .interpolate(&[[0.0, 0.0]], &[7.0], &[[0.5, 0.0], [10.0, 0.0]])
            .unwrap();
        assert!((answer.values[0] - 7.0).abs() < 1e-12);
        assert!(answer.values[1].is_nan());
        assert_eq!(answer.unreached, 1);
    }

    #[test]
    fn the_k_nearest_are_the_ones_that_take_part() {
        // With k = 1 the answer is the nearest sample's value exactly.
        let samples = [[0.0, 0.0], [10.0, 0.0]];
        let answer = Idw::new(2.0)
            .with_neighbours(1)
            .interpolate(&samples, &[1.0, 100.0], &[[2.0, 0.0]])
            .unwrap();
        assert_eq!(answer.values, vec![1.0]);
    }

    #[test]
    fn three_dimensions_work_the_same_way() {
        let samples = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let answer = Idw::new(2.0)
            .interpolate(&samples, &[1.0, 3.0], &[[1.0, 0.0, 0.0]])
            .unwrap();
        assert!((answer.values[0] - 2.0).abs() < 1e-12);
    }

    #[test]
    fn every_refusal_is_named() {
        let s = [[0.0, 0.0]];
        assert_eq!(
            Idw::new(2.0).interpolate(&s, &[1.0, 2.0], &[]),
            Err(Error::MismatchedLengths { count: 1, values: 2 })
        );
        assert_eq!(
            Idw::new(0.0).interpolate(&s, &[1.0], &[]),
            Err(Error::BadPower { given: 0.0 })
        );
        assert_eq!(
            Idw::new(2.0).with_radius(-1.0).interpolate(&s, &[1.0], &[]),
            Err(Error::BadRadius { given: -1.0 })
        );
        assert_eq!(
            Idw::new(2.0).with_neighbours(0).interpolate(&s, &[1.0], &[]),
            Err(Error::NoNeighbours)
        );
        assert_eq!(
            Idw::new(2.0).interpolate(&s, &[f64::NAN], &[[0.0, 0.0]]),
            Err(Error::NoSample {
                given: 1,
                set_aside: 1
            })
        );
        assert_eq!(
            Idw::new(2.0).interpolate::<2>(&[], &[], &[[0.0, 0.0]]),
            Err(Error::NoSample {
                given: 0,
                set_aside: 0
            })
        );
    }
}
