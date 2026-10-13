//! Ordinary kriging — the estimator that says how sure it is.
//!
//! Where IDW weights by distance alone, kriging weights by the fitted
//! [`Model`](crate::variogram::Model): samples beyond the range stop
//! mattering, clustered samples share their influence instead of counting
//! twice, and every estimate comes with its **kriging variance** — the
//! model's own statement of uncertainty, which is the reason to pay for
//! kriging at all.
//!
//! The system is dense and solved once: the kriging matrix depends only on
//! the samples, so it is LU-factorised a single time and each query is one
//! back-substitution — O(n³) once, O(n²) per query, honest for the
//! hundreds of wells this serves.
//!
//! # Global, or by neighbourhood
//!
//! That single factorisation holds only while every query sees every sample.
//! A search radius, a cap on the neighbours, or a fault as a barrier make the
//! sample set **depend on the query** — kriging with fault lines
//! is of that kind — and then each query assembles and solves its own small
//! system. Same estimator, same variance formula; only the set of samples in
//! the sum changes. A query no sample reaches is `NaN` and counted, like an
//! IDW radius nobody reaches. The two paths are cross-checked against each
//! other in the tests: with no radius, no cap and no fault, the local path
//! must reproduce the global one to the last digit.
//!
//! Two refusals worth naming: two samples
//! on the **same position** make the matrix singular by construction
//! (refused up front, naming both indices — the fix is deduplication, and
//! choosing which value wins is the caller's call, not this crate's); and
//! a system the factorisation finds singular anyway refuses rather than
//! returning the garbage a zero pivot produces.
//!
//! TODO(reference): verify — ordinary kriging follows standard geostatistics; no publication is cited in this code. Add a reference once checked.

use crate::Error;
use crate::barrier::{Barriers, keep_visible};
use crate::idw::plan;
use crate::solve::Lu;
use crate::variogram::Model;

/// Ordinary kriging over a variogram model.
#[derive(Debug, Clone)]
pub struct Ordinary {
    pub model: Model,
    /// A query closer than this to a sample *is* that sample — same rule
    /// and same default as [`crate::Idw`].
    pub exact_within: f64,
    /// Only samples within this distance of the query take part. `None` =
    /// all of them.
    pub radius: Option<f64>,
    /// Only the `k` nearest samples take part. `None` = all of them.
    pub neighbours: Option<usize>,
    /// Faults the estimator must not see across — see [`Barriers`].
    pub barriers: Option<Barriers>,
    /// Whether the kriging variance is wanted. `true` by default; a caller
    /// that writes only the estimates says so with [`Ordinary::without_variance`],
    /// and the global path then costs one term per sample and query instead
    /// of one solve.
    pub variance: bool,
}

/// What a kriging run answers.
#[derive(Debug, Clone, PartialEq)]
pub struct KrigingAnswer {
    /// One estimate per query, in order.
    pub values: Vec<f64>,
    /// The kriging variance of each estimate — 0 on a sample, growing
    /// towards `sill` (and past it, by the Lagrange term) far from data.
    /// `NaN` where the estimate is. **Empty** when the variance was not asked
    /// for ([`Ordinary::without_variance`]) — an absence, not a list of zeros.
    pub variances: Vec<f64>,
    /// Samples set aside for a `NaN` value or coordinate.
    pub set_aside: usize,
    /// Queries no sample could reach — behind a fault, or past the radius.
    /// Each is a `NaN` in `values`.
    pub unreached: usize,
}

impl Ordinary {
    pub fn new(model: Model) -> Self {
        Ordinary {
            model,
            exact_within: 1e-12,
            radius: None,
            neighbours: None,
            barriers: None,
            variance: true,
        }
    }

    /// Only the estimates: no variance is computed, and `variances` comes
    /// back empty.
    ///
    /// # Why the estimates alone are cheaper, and the same
    ///
    /// The ordinary kriging matrix `M` is symmetric, so the estimate
    /// `wᵀz`, with `M [w; μ] = [γ₀; 1]`, is also `aᵀ [γ₀; 1]` with
    /// `M a = [z; 0]` — the **dual** form. `a` is solved once for all queries,
    /// and each query then costs one variogram term per sample instead of a
    /// solve in `n²`. Measured by a speed report: 20 s for 1 000 samples
    /// onto 10 000 nodes, the variance nobody wrote included.
    /// The two forms are confronted in
    /// the tests. The variance, which needs the weights themselves, has no
    /// such shortcut — hence a switch rather than a change.
    #[must_use]
    pub fn without_variance(mut self) -> Self {
        self.variance = false;
        self
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

    /// Whether the sample set depends on the query.
    fn is_local(&self) -> bool {
        self.radius.is_some()
            || self.neighbours.is_some()
            || self.barriers.as_ref().is_some_and(|b| !b.is_empty())
    }

    /// Estimate `values` known at `samples` onto `queries`.
    pub fn interpolate<const N: usize>(
        &self,
        samples: &[[f64; N]],
        values: &[f64],
        queries: &[[f64; N]],
    ) -> Result<KrigingAnswer, Error> {
        if samples.len() != values.len() {
            return Err(Error::MismatchedLengths {
                count: samples.len(),
                values: values.len(),
            });
        }
        if let Some(radius) = self.radius {
            if !(radius.is_finite() && radius > 0.0) {
                return Err(Error::BadRadius { given: radius });
            }
        }
        if self.neighbours == Some(0) {
            return Err(Error::NoNeighbours);
        }
        let usable: Vec<([f64; N], f64)> = samples
            .iter()
            .zip(values)
            .filter(|(point, value)| value.is_finite() && point.iter().all(|c| c.is_finite()))
            .map(|(point, value)| (*point, *value))
            .collect();
        let set_aside = samples.len() - usable.len();
        if usable.is_empty() {
            return Err(Error::NoSample {
                given: samples.len(),
                set_aside,
            });
        }

        let distance = |a: &[f64; N], b: &[f64; N]| -> f64 {
            a.iter()
                .zip(b)
                .map(|(x, y)| (x - y) * (x - y))
                .sum::<f64>()
                .sqrt()
        };
        // Coincident samples make two identical rows — singular by
        // construction, and better refused with both names than found as
        // a mysterious zero pivot below.
        for (at, (a, _)) in usable.iter().enumerate() {
            for (offset, (b, _)) in usable[at + 1..].iter().enumerate() {
                if distance(a, b) <= self.exact_within {
                    return Err(Error::DuplicateSamplePositions {
                        first: at,
                        second: at + 1 + offset,
                    });
                }
            }
        }

        if self.is_local() {
            return self.local(&usable, queries, set_aside, distance);
        }

        // The ordinary kriging matrix: γ between samples, bordered by the
        // Lagrange row/column that makes the weights sum to 1.
        let n = usable.len();
        let side = n + 1;
        let mut matrix = vec![0.0f64; side * side];
        for i in 0..n {
            for j in 0..n {
                matrix[i * side + j] = self.model.evaluate(distance(&usable[i].0, &usable[j].0));
            }
            matrix[i * side + n] = 1.0;
            matrix[n * side + i] = 1.0;
        }
        let factored = Lu::factor(matrix, side)?;

        if !self.variance {
            // The dual form — see `without_variance`.
            let mut rhs: Vec<f64> = usable.iter().map(|(_, z)| *z).collect();
            rhs.push(0.0);
            let dual = factored.solve(&rhs);
            let values = queries
                .iter()
                .map(|query| {
                    if let Some((_, value)) = usable
                        .iter()
                        .find(|(point, _)| distance(point, query) <= self.exact_within)
                    {
                        return *value;
                    }
                    usable
                        .iter()
                        .zip(&dual)
                        .map(|((point, _), a)| a * self.model.evaluate(distance(point, query)))
                        .sum::<f64>()
                        + dual[n]
                })
                .collect();
            return Ok(KrigingAnswer {
                values,
                variances: Vec::new(),
                set_aside,
                unreached: 0,
            });
        }

        let mut out = Vec::with_capacity(queries.len());
        let mut variances = Vec::with_capacity(queries.len());
        let mut rhs = vec![0.0f64; side];
        for query in queries {
            if let Some((_, value)) = usable
                .iter()
                .find(|(point, _)| distance(point, query) <= self.exact_within)
            {
                out.push(*value);
                variances.push(0.0);
                continue;
            }
            for (slot, (point, _)) in rhs.iter_mut().zip(&usable) {
                *slot = self.model.evaluate(distance(point, query));
            }
            rhs[n] = 1.0;
            let weights = factored.solve(&rhs);
            let estimate: f64 = weights[..n].iter().zip(&usable).map(|(w, (_, z))| w * z).sum();
            // σ² = Σ wᵢ γ(dᵢ) + μ — the Lagrange multiplier is weights[n].
            let variance: f64 = weights[..n]
                .iter()
                .zip(&rhs[..n])
                .map(|(w, g)| w * g)
                .sum::<f64>()
                + weights[n];
            out.push(estimate);
            variances.push(variance.max(0.0));
        }
        Ok(KrigingAnswer {
            values: out,
            variances,
            set_aside,
            unreached: 0,
        })
    }

    /// The by-neighbourhood path: one small system per query.
    ///
    /// The neighbourhood is decided in this order — exact hit, then the
    /// fault, then the radius, then the `k` nearest — and the order matters
    /// only for the last: the nearest `k` are taken among the samples that
    /// are already visible and in reach, never the other way round, or a
    /// wall of near samples behind a fault would crowd out every visible one.
    fn local<const N: usize>(
        &self,
        usable: &[([f64; N], f64)],
        queries: &[[f64; N]],
        set_aside: usize,
        distance: impl Fn(&[f64; N], &[f64; N]) -> f64,
    ) -> Result<KrigingAnswer, Error> {
        let mut out = Vec::with_capacity(queries.len());
        let mut variances = Vec::with_capacity(queries.len());
        let mut unreached = 0usize;
        // Reused across queries: (distance, index into `usable`).
        let mut near: Vec<(f64, usize)> = Vec::with_capacity(usable.len());
        for query in queries {
            near.clear();
            let mut exact: Option<f64> = None;
            for (at, (point, value)) in usable.iter().enumerate() {
                let d = distance(point, query);
                if d <= self.exact_within {
                    exact = Some(*value);
                    break;
                }
                if self.radius.is_none_or(|r| d <= r) {
                    near.push((d, at));
                }
            }
            if let Some(value) = exact {
                out.push(value);
                variances.push(0.0);
                continue;
            }
            // The fault after the radius (cheap first), the cap after the
            // fault (the rule) — see `keep_visible`.
            match &self.barriers {
                Some(faults) => keep_visible(&mut near, self.neighbours, |at| {
                    faults.blocks(plan(&usable[at].0), plan(query))
                }),
                None => {
                    if let Some(k) = self.neighbours {
                        if near.len() > k {
                            near.select_nth_unstable_by(k - 1, |a, b| a.0.total_cmp(&b.0));
                            near.truncate(k);
                        }
                    }
                }
            }
            if near.is_empty() {
                out.push(f64::NAN);
                variances.push(f64::NAN);
                unreached += 1;
                continue;
            }
            // The same bordered system as the global path, over the
            // neighbourhood only.
            let n = near.len();
            let side = n + 1;
            let mut matrix = vec![0.0f64; side * side];
            for i in 0..n {
                for j in 0..n {
                    matrix[i * side + j] = self
                        .model
                        .evaluate(distance(&usable[near[i].1].0, &usable[near[j].1].0));
                }
                matrix[i * side + n] = 1.0;
                matrix[n * side + i] = 1.0;
            }
            let factored = Lu::factor(matrix, side)?;
            let mut rhs = vec![0.0f64; side];
            for (slot, (d, _)) in rhs.iter_mut().zip(&near) {
                *slot = self.model.evaluate(*d);
            }
            rhs[n] = 1.0;
            let weights = factored.solve(&rhs);
            let estimate: f64 = weights[..n]
                .iter()
                .zip(&near)
                .map(|(w, (_, at))| w * usable[*at].1)
                .sum();
            out.push(estimate);
            if self.variance {
                let variance: f64 = weights[..n]
                    .iter()
                    .zip(&rhs[..n])
                    .map(|(w, g)| w * g)
                    .sum::<f64>()
                    + weights[n];
                variances.push(variance.max(0.0));
            }
        }
        Ok(KrigingAnswer {
            values: out,
            variances,
            set_aside,
            unreached,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variogram::ModelKind;

    fn model() -> Model {
        Model::new(ModelKind::Spherical, 0.0, 1.0, 10.0).unwrap()
    }

    /// **The dual form gives the estimates of the primal one** — on scattered
    /// samples, onto queries between, beside and far from them, for each
    /// model. And it gives no variance, rather than zeros.
    #[test]
    fn the_estimates_alone_are_those_of_the_full_solve() {
        let mut state = 5u64;
        let mut draw = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let samples: Vec<[f64; 2]> = (0..120).map(|_| [draw() * 100.0, draw() * 100.0]).collect();
        let values: Vec<f64> = samples
            .iter()
            .map(|p| (p[0] / 17.0).sin() * 30.0 + p[1] * 0.2)
            .collect();
        let mut queries: Vec<[f64; 2]> = (0..300)
            .map(|_| [draw() * 140.0 - 20.0, draw() * 140.0 - 20.0])
            .collect();
        queries.push(samples[7]);
        for kind in [ModelKind::Spherical, ModelKind::Exponential] {
            let model = Model::new(kind, 0.5, 40.0, 35.0).unwrap();
            let full = Ordinary::new(model.clone())
                .interpolate(&samples, &values, &queries)
                .unwrap();
            let alone = Ordinary::new(model)
                .without_variance()
                .interpolate(&samples, &values, &queries)
                .unwrap();
            assert!(alone.variances.is_empty() && full.variances.len() == queries.len());
            for (at, (a, b)) in alone.values.iter().zip(&full.values).enumerate() {
                assert!(
                    (a - b).abs() <= 1e-9 * (1.0 + b.abs()),
                    "{kind:?}, query {at}: {a} against {b}"
                );
            }
            assert_eq!(
                alone.values.last(),
                Some(&values[7]),
                "a query on a sample is that sample"
            );
        }
    }

    #[test]
    fn kriging_is_exact_at_its_samples_with_zero_variance() {
        let samples = [[0.0, 0.0], [4.0, 0.0], [2.0, 3.0]];
        let values = [1.0, 2.0, 3.0];
        let answer = Ordinary::new(model())
            .interpolate(&samples, &values, &samples)
            .unwrap();
        assert_eq!(answer.values, vec![1.0, 2.0, 3.0]);
        assert_eq!(answer.variances, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_constant_field_is_reproduced_with_the_weights_summing_to_one() {
        // Ordinary kriging's defining constraint: Σw = 1, so a constant
        // field comes back constant — anywhere, any model.
        let samples = [[0.0, 0.0], [5.0, 1.0], [2.0, 8.0], [7.0, 6.0]];
        let values = [4.2; 4];
        let queries = [[3.0, 3.0], [-10.0, -10.0], [100.0, 0.0]];
        let answer = Ordinary::new(model())
            .interpolate(&samples, &values, &queries)
            .unwrap();
        for value in &answer.values {
            assert!((value - 4.2).abs() < 1e-9);
        }
    }

    #[test]
    fn the_midpoint_of_two_samples_is_their_mean() {
        // Symmetry: both weights are ½ whatever the model says.
        let answer = Ordinary::new(model())
            .interpolate(&[[0.0, 0.0], [2.0, 0.0]], &[1.0, 3.0], &[[1.0, 0.0]])
            .unwrap();
        assert!((answer.values[0] - 2.0).abs() < 1e-9);
    }

    /// **The two paths are one estimator.** With a cap larger than the
    /// sample count, no radius and no fault, the by-neighbourhood path must
    /// reproduce the global factorisation to the last digit — values and
    /// variances. A local assembly wrong by one index would still return
    /// plausible numbers; this is what catches it.
    #[test]
    fn the_local_path_reproduces_the_global_one_when_nothing_restricts_it() {
        let samples = [[0.0, 0.0], [5.0, 1.0], [2.0, 8.0], [7.0, 6.0], [3.0, 3.5]];
        let values = [1.0, 4.0, 2.5, 6.0, 3.0];
        let queries: Vec<[f64; 2]> = (0..20)
            .map(|i| [(i as f64) * 0.4, ((i * 3) % 9) as f64])
            .collect();
        let global = Ordinary::new(model())
            .interpolate(&samples, &values, &queries)
            .unwrap();
        let local = Ordinary::new(model())
            .with_neighbours(50)
            .interpolate(&samples, &values, &queries)
            .unwrap();
        for (g, l) in global.values.iter().zip(&local.values) {
            assert!((g - l).abs() < 1e-9, "global {g} vs local {l}");
        }
        for (g, l) in global.variances.iter().zip(&local.variances) {
            assert!((g - l).abs() < 1e-9, "variance global {g} vs local {l}");
        }
        assert_eq!(local.unreached, 0);
    }

    /// A fault is a barrier: the query a hair from the far sample hears only
    /// the near ones — the same case as IDW's, on the other estimator.
    #[test]
    fn a_sample_behind_a_fault_takes_no_part() {
        let fault = Barriers::from_polylines(&[vec![[5.0, -10.0], [5.0, 10.0]]]).unwrap();
        let samples = [[0.0, 0.0], [1.0, 1.0], [6.0, 0.0]];
        let values = [1.0, 1.0, 100.0];
        let query = [[4.9, 0.0]];
        let seen = Ordinary::new(model())
            .interpolate(&samples, &values, &query)
            .unwrap()
            .values[0];
        let cut = Ordinary::new(model())
            .with_barriers(fault)
            .interpolate(&samples, &values, &query)
            .unwrap()
            .values[0];
        assert!(seen > 50.0, "without the fault the near sample dominates: {seen}");
        assert!(
            (cut - 1.0).abs() < 1e-9,
            "with the fault only the visible samples answer: {cut}"
        );
    }

    #[test]
    fn a_query_walled_off_or_out_of_radius_is_unreached_not_a_refusal() {
        let fault = Barriers::from_polylines(&[vec![[5.0, -10.0], [5.0, 10.0]]]).unwrap();
        let answer = Ordinary::new(model())
            .with_barriers(fault)
            .interpolate(&[[0.0, 0.0], [1.0, 1.0]], &[1.0, 2.0], &[[9.0, 0.0]])
            .unwrap();
        assert!(answer.values[0].is_nan() && answer.variances[0].is_nan());
        assert_eq!(answer.unreached, 1);

        let answer = Ordinary::new(model())
            .with_radius(1.0)
            .interpolate(&[[0.0, 0.0], [1.0, 1.0]], &[1.0, 2.0], &[[0.5, 0.0], [50.0, 0.0]])
            .unwrap();
        assert!(answer.values[0].is_finite());
        assert!(answer.values[1].is_nan());
        assert_eq!(answer.unreached, 1);
    }

    /// The nearest `k` are taken among the **visible** samples, never the
    /// other way round: a wall of near samples behind a fault must not crowd
    /// out the one sample the query can actually see.
    #[test]
    fn the_cap_applies_after_the_fault_not_before() {
        let fault = Barriers::from_polylines(&[vec![[5.0, -10.0], [5.0, 10.0]]]).unwrap();
        let samples = [[6.0, 0.0], [6.0, 0.5], [6.0, -0.5], [0.0, 0.0]];
        let values = [100.0, 100.0, 100.0, 1.0];
        let answer = Ordinary::new(model())
            .with_barriers(fault)
            .with_neighbours(2)
            .interpolate(&samples, &values, &[[4.9, 0.0]])
            .unwrap();
        assert!((answer.values[0] - 1.0).abs() < 1e-9, "{}", answer.values[0]);
    }

    #[test]
    fn far_from_every_sample_the_estimate_tends_to_the_mean() {
        // Beyond the range every γ is the sill: the system flattens and
        // ordinary kriging hands back the sample mean — the property that
        // distinguishes it from IDW, which chases the nearest value. The
        // samples must be **mutually** uncorrelated too (farther than the
        // range from each other): clustered samples get GLS weights, and
        // the far-field limit is then the GLS mean, not the arithmetic one.
        let samples = [[0.0, 0.0], [100.0, 0.0], [0.0, 100.0]];
        let values = [1.0, 2.0, 6.0];
        let answer = Ordinary::new(model())
            .interpolate(&samples, &values, &[[500.0, 500.0]])
            .unwrap();
        assert!((answer.values[0] - 3.0).abs() < 1e-6, "got {}", answer.values[0]);
    }

    #[test]
    fn the_variance_grows_with_distance_from_the_data() {
        let samples = [[0.0, 0.0], [1.0, 0.0]];
        let values = [1.0, 2.0];
        let queries = [[0.5, 0.0], [0.5, 3.0], [0.5, 30.0]];
        let answer = Ordinary::new(model())
            .interpolate(&samples, &values, &queries)
            .unwrap();
        assert!(answer.variances[0] < answer.variances[1]);
        assert!(answer.variances[1] < answer.variances[2]);
    }

    #[test]
    fn a_two_sample_system_matches_the_closed_form() {
        // n = 2 solves by hand: w₁ − w₂ = (γ₂q − γ₁q) / (2 γ₁₂) around ½.
        let m = model();
        let samples = [[0.0, 0.0], [4.0, 0.0]];
        let values = [0.0, 1.0];
        let query = [[1.0, 0.0]];
        let g1q = m.evaluate(1.0);
        let g2q = m.evaluate(3.0);
        let g12 = m.evaluate(4.0);
        let w2 = 0.5 - (g2q - g1q) / (2.0 * g12);
        let answer = Ordinary::new(m).interpolate(&samples, &values, &query).unwrap();
        assert!(
            (answer.values[0] - w2).abs() < 1e-9,
            "{} vs {}",
            answer.values[0],
            w2
        );
    }

    #[test]
    fn a_nan_sample_is_set_aside_and_counted() {
        let samples = [[0.0, 0.0], [2.0, 0.0], [1.0, 4.0]];
        let values = [1.0, 3.0, f64::NAN];
        let answer = Ordinary::new(model())
            .interpolate(&samples, &values, &[[1.0, 0.0]])
            .unwrap();
        assert_eq!(answer.set_aside, 1);
        assert!((answer.values[0] - 2.0).abs() < 1e-9);
    }

    #[test]
    fn coincident_samples_are_refused_naming_both() {
        let samples = [[0.0, 0.0], [2.0, 0.0], [0.0, 0.0]];
        let refusal = Ordinary::new(model()).interpolate(&samples, &[1.0, 2.0, 3.0], &[[1.0, 0.0]]);
        assert_eq!(
            refusal,
            Err(Error::DuplicateSamplePositions { first: 0, second: 2 })
        );
    }

    #[test]
    fn three_dimensions_work_the_same_way() {
        let samples = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let answer = Ordinary::new(model())
            .interpolate(&samples, &[1.0, 3.0], &[[1.0, 0.0, 0.0]])
            .unwrap();
        assert!((answer.values[0] - 2.0).abs() < 1e-9);
    }
}
