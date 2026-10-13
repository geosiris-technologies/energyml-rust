//! The empirical variogram, and the models fitted over it.
//!
//! Kriging needs a statement of *how fast the field decorrelates with
//! distance*; the empirical variogram measures it from the data and a
//! model makes it usable at every distance. The split matters: the
//! empirical bins are arithmetic over the samples (testable exactly), the
//! model is a choice — and a fitted model says which choice and with what
//! parameters, instead of burying them.
//!
//! Conventions worth stating, because both have a silent wrong reading:
//! - `range` is the **practical** range for the exponential model — the
//!   distance where the model reaches ~95 % of the sill (`1 − e⁻³`), which
//!   is what most geostatistics software reports. The raw exponential
//!   parameter is `range / 3`.
//! - `γ(0) = 0` always. A nugget is the *limit* of `γ` as `h → 0⁺`, not
//!   its value at zero — this is what keeps kriging exact at its samples.
//!
//! TODO(reference): verify — the empirical variogram and its models follow standard geostatistics; no publication is cited in this code. Add a reference once checked.

use crate::Error;

/// One distance class of the empirical variogram.
#[derive(Debug, Clone, PartialEq)]
pub struct Bin {
    /// The mean pair distance of the class — not the class centre, the
    /// distance the pairs actually have; fitting against the centre biases
    /// the model where pairs cluster near an edge.
    pub distance: f64,
    /// The semivariance `(1 / 2n) Σ (zᵢ − zⱼ)²` over the class's pairs.
    pub semivariance: f64,
    /// How many pairs the class holds — the weight of the class in a fit.
    pub pairs: usize,
}

/// What [`empirical`] answers: the bins, and the honesty counter.
#[derive(Debug, Clone, PartialEq)]
pub struct Empirical {
    /// Only classes that hold at least one pair — an empty class says
    /// nothing and would fit as if it said zero.
    pub bins: Vec<Bin>,
    /// Samples set aside for a `NaN` value or coordinate.
    pub set_aside: usize,
}

/// The empirical variogram of `values` sampled at `samples`.
///
/// Pairs farther apart than `max_distance` are left out; the default is
/// **half the largest pair distance**, the standard practice — beyond it
/// the classes hold few pairs and mostly noise. `classes` is how many
/// distance classes divide `[0, max_distance]`.
pub fn empirical<const N: usize>(
    samples: &[[f64; N]],
    values: &[f64],
    max_distance: Option<f64>,
    classes: usize,
) -> Result<Empirical, Error> {
    if samples.len() != values.len() {
        return Err(Error::MismatchedLengths {
            count: samples.len(),
            values: values.len(),
        });
    }
    if classes == 0 {
        return Err(Error::NoClasses);
    }
    if let Some(reach) = max_distance {
        if !(reach.is_finite() && reach > 0.0) {
            return Err(Error::BadRadius { given: reach });
        }
    }
    let usable: Vec<([f64; N], f64)> = samples
        .iter()
        .zip(values)
        .filter(|(point, value)| value.is_finite() && point.iter().all(|c| c.is_finite()))
        .map(|(point, value)| (*point, *value))
        .collect();
    let set_aside = samples.len() - usable.len();
    if usable.len() < 2 {
        return Err(Error::TooFewSamples {
            usable: usable.len(),
            needed: 2,
        });
    }

    let distance = |a: &[f64; N], b: &[f64; N]| -> f64 {
        a.iter()
            .zip(b)
            .map(|(x, y)| (x - y) * (x - y))
            .sum::<f64>()
            .sqrt()
    };
    let farthest = {
        let mut top = 0.0f64;
        for (at, (a, _)) in usable.iter().enumerate() {
            for (b, _) in &usable[at + 1..] {
                top = top.max(distance(a, b));
            }
        }
        top
    };
    if farthest <= 0.0 {
        // Every usable sample sits on one point — no distance, no variogram.
        return Err(Error::DuplicateSamplePositions { first: 0, second: 1 });
    }
    let reach = max_distance.unwrap_or(farthest / 2.0);

    let mut sums = vec![(0.0f64, 0.0f64, 0usize); classes]; // (Σd, Σ(Δz)², n)
    for (at, (a, za)) in usable.iter().enumerate() {
        for (b, zb) in &usable[at + 1..] {
            let d = distance(a, b);
            if d > reach {
                continue;
            }
            let class = (((d / reach) * classes as f64) as usize).min(classes - 1);
            let slot = &mut sums[class];
            slot.0 += d;
            slot.1 += (za - zb) * (za - zb);
            slot.2 += 1;
        }
    }
    let bins: Vec<Bin> = sums
        .into_iter()
        .filter(|(_, _, n)| *n > 0)
        .map(|(d, dz2, n)| Bin {
            distance: d / n as f64,
            semivariance: dz2 / (2.0 * n as f64),
            pairs: n,
        })
        .collect();
    if bins.is_empty() {
        return Err(Error::TooFewPairs { reach });
    }
    Ok(Empirical { bins, set_aside })
}

/// The shape of a variogram model — the registry-style list this crate
/// evaluates. A new shape is a new variant plus one arm in `shape`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    /// Reaches the sill exactly at `range`.
    Spherical,
    /// Approaches the sill asymptotically; `range` is the practical range.
    Exponential,
}

/// A fitted (or hand-stated) variogram model: `γ(h)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub kind: ModelKind,
    /// The discontinuity at the origin — measurement noise plus
    /// variability below the sampling distance. `γ(0)` itself is 0.
    pub nugget: f64,
    /// The plateau: the variance the field settles at, far away.
    pub sill: f64,
    /// Where the plateau is reached (spherical) or practically reached
    /// (exponential, ~95 %).
    pub range: f64,
}

impl Model {
    /// A model stated by hand — refused by name when the numbers cannot
    /// be a variogram (negative nugget, sill below nugget, range ≤ 0).
    pub fn new(kind: ModelKind, nugget: f64, sill: f64, range: f64) -> Result<Model, Error> {
        if !(nugget.is_finite() && nugget >= 0.0) {
            return Err(Error::BadModel {
                what: format!("nugget {nugget}"),
            });
        }
        if !(sill.is_finite() && sill >= nugget) {
            return Err(Error::BadModel {
                what: format!("sill {sill} below nugget {nugget}"),
            });
        }
        if !(range.is_finite() && range > 0.0) {
            return Err(Error::BadModel {
                what: format!("range {range}"),
            });
        }
        Ok(Model {
            kind,
            nugget,
            sill,
            range,
        })
    }

    /// The structured part of the model, rising from 0 to 1 over `range`.
    fn shape(&self, h: f64) -> f64 {
        let r = h / self.range;
        match self.kind {
            ModelKind::Spherical => {
                if r >= 1.0 {
                    1.0
                } else {
                    1.5 * r - 0.5 * r * r * r
                }
            }
            // The practical-range convention: 1 − e⁻³ ≈ 0.95 at h = range.
            ModelKind::Exponential => 1.0 - (-3.0 * r).exp(),
        }
    }

    /// `γ(h)` — zero at zero, nugget-limited above it, sill far away.
    pub fn evaluate(&self, h: f64) -> f64 {
        if h <= 0.0 {
            return 0.0;
        }
        self.nugget + (self.sill - self.nugget) * self.shape(h)
    }

    /// Fit a model of `kind` to an empirical variogram, by weighted least
    /// squares — each class weighted by its pair count, so a class of
    /// three pairs does not bend the model like a class of three hundred.
    ///
    /// The range is searched over a grid spanning the observed distances
    /// (the fit is one-dimensional and cheap); for each candidate range
    /// the best `(nugget, sill)` is a linear least-squares solve, clamped
    /// to what a variogram allows. Deterministic, no starting point.
    pub fn fit(observed: &Empirical, kind: ModelKind) -> Result<Model, Error> {
        let bins = &observed.bins;
        if bins.len() < 3 {
            return Err(Error::TooFewClasses {
                held: bins.len(),
                needed: 3,
            });
        }
        let shortest = bins.iter().map(|b| b.distance).fold(f64::MAX, f64::min);
        let longest = bins.iter().map(|b| b.distance).fold(0.0f64, f64::max);

        let mut best: Option<(f64, Model)> = None;
        for step in 0..64 {
            // Log-spaced candidates from the shortest class to past the
            // longest — a range beyond the data is a legitimate answer
            // (the field decorrelates slower than the survey is wide).
            let t = step as f64 / 63.0;
            let range = shortest * (2.0 * longest / shortest).powf(t);
            let candidate = ModelKind::best_linear(kind, bins, range);
            let sse: f64 = bins
                .iter()
                .map(|b| {
                    let miss = candidate.evaluate(b.distance) - b.semivariance;
                    b.pairs as f64 * miss * miss
                })
                .sum();
            if best.as_ref().is_none_or(|(top, _)| sse < *top) {
                best = Some((sse, candidate));
            }
        }
        Ok(best.expect("64 candidates were scored").1)
    }
}

impl ModelKind {
    /// For a fixed range, the weighted least-squares `(nugget, sill)` —
    /// the model is linear in both once the shape is known. Clamped to
    /// nugget ≥ 0 and sill ≥ nugget, because a fit outside those is not a
    /// variogram and the clamp is re-solved, not just cut.
    fn best_linear(kind: ModelKind, bins: &[Bin], range: f64) -> Model {
        // γ = nugget · 1 + spread · shape(h): ordinary 2×2 normal equations.
        let probe = Model {
            kind,
            nugget: 0.0,
            sill: 1.0,
            range,
        };
        let (mut s_ww, mut s_ws, mut s_ss, mut s_wy, mut s_sy) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for bin in bins {
            let w = bin.pairs as f64;
            let s = probe.shape(bin.distance);
            s_ww += w;
            s_ws += w * s;
            s_ss += w * s * s;
            s_wy += w * bin.semivariance;
            s_sy += w * s * bin.semivariance;
        }
        let determinant = s_ww * s_ss - s_ws * s_ws;
        let (mut nugget, mut spread) = if determinant.abs() > 1e-12 {
            (
                (s_wy * s_ss - s_sy * s_ws) / determinant,
                (s_sy * s_ww - s_wy * s_ws) / determinant,
            )
        } else {
            (0.0, if s_ss > 0.0 { s_sy / s_ss } else { 0.0 })
        };
        if nugget < 0.0 {
            // Re-solve with nugget pinned at 0 — dropping the constraint's
            // row, not clipping the unconstrained answer.
            nugget = 0.0;
            spread = if s_ss > 0.0 { s_sy / s_ss } else { 0.0 };
        }
        if spread < 0.0 {
            spread = 0.0;
            nugget = if s_ww > 0.0 { s_wy / s_ww } else { 0.0 };
        }
        Model {
            kind,
            nugget,
            sill: nugget + spread,
            range,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nine samples on a line, values = x: every pair at distance h has
    /// (Δz)² = h², so γ(h) = h²/2 exactly — arithmetic, not statistics.
    fn line() -> (Vec<[f64; 2]>, Vec<f64>) {
        let samples: Vec<[f64; 2]> = (0..9).map(|at| [at as f64, 0.0]).collect();
        let values = samples.iter().map(|p| p[0]).collect();
        (samples, values)
    }

    #[test]
    fn the_empirical_variogram_of_a_linear_field_is_h_squared_over_two() {
        let (samples, values) = line();
        // Reach 4.5 puts each integer distance in its own right-open
        // class — reach 4.0 would clamp d = 4 into d = 3's class and mix
        // two distances in one bin.
        let observed = empirical(&samples, &values, Some(4.5), 4).unwrap();
        for bin in &observed.bins {
            let expected = bin.distance * bin.distance / 2.0;
            assert!(
                (bin.semivariance - expected).abs() < 1e-9,
                "γ({}) = {} where {} was expected",
                bin.distance,
                bin.semivariance,
                expected
            );
        }
        // Distance 1 has 8 pairs, distance 4 has 5 — the counts are data.
        assert_eq!(observed.bins[0].pairs, 8);
        assert_eq!(observed.bins[3].pairs, 5);
    }

    #[test]
    fn a_nan_sample_is_set_aside_and_counted() {
        let (mut samples, mut values) = line();
        samples.push([100.0, 0.0]);
        values.push(f64::NAN);
        let observed = empirical(&samples, &values, Some(4.0), 4).unwrap();
        assert_eq!(observed.set_aside, 1);
    }

    #[test]
    fn the_default_reach_is_half_the_farthest_pair() {
        let (samples, values) = line();
        let observed = empirical(&samples, &values, None, 4).unwrap();
        assert!(observed.bins.iter().all(|b| b.distance <= 4.0));
    }

    #[test]
    fn models_hold_their_conventions() {
        let spherical = Model::new(ModelKind::Spherical, 0.5, 2.0, 10.0).unwrap();
        assert_eq!(spherical.evaluate(0.0), 0.0, "γ(0) is 0, the nugget is a limit");
        assert!(
            (spherical.evaluate(1e-9) - 0.5).abs() < 1e-6,
            "the limit is the nugget"
        );
        assert_eq!(spherical.evaluate(10.0), 2.0, "the sill is reached at the range");
        assert_eq!(spherical.evaluate(50.0), 2.0, "and held beyond it");

        let exponential = Model::new(ModelKind::Exponential, 0.0, 1.0, 10.0).unwrap();
        let at_range = exponential.evaluate(10.0);
        assert!(
            (at_range - 0.95).abs() < 0.01,
            "practical range: ~95 % of the sill"
        );
    }

    #[test]
    fn the_fit_recovers_the_model_that_made_the_bins() {
        let truth = Model::new(ModelKind::Spherical, 0.2, 1.7, 6.0).unwrap();
        let observed = Empirical {
            bins: (1..=10)
                .map(|at| {
                    let h = at as f64;
                    Bin {
                        distance: h,
                        semivariance: truth.evaluate(h),
                        pairs: 50,
                    }
                })
                .collect(),
            set_aside: 0,
        };
        let fitted = Model::fit(&observed, ModelKind::Spherical).unwrap();
        assert!((fitted.nugget - 0.2).abs() < 0.05, "nugget: {}", fitted.nugget);
        assert!((fitted.sill - 1.7).abs() < 0.05, "sill: {}", fitted.sill);
        assert!((fitted.range - 6.0).abs() < 0.6, "range: {}", fitted.range);
    }

    #[test]
    fn the_fit_never_answers_a_negative_nugget() {
        // A concave-down start (spherical-looking, no nugget) must not be
        // fitted with nugget < 0 to buy a better sum of squares.
        let truth = Model::new(ModelKind::Spherical, 0.0, 1.0, 5.0).unwrap();
        let observed = Empirical {
            bins: (1..=8)
                .map(|at| {
                    let h = at as f64;
                    Bin {
                        distance: h,
                        semivariance: truth.evaluate(h),
                        pairs: 10,
                    }
                })
                .collect(),
            set_aside: 0,
        };
        let fitted = Model::fit(&observed, ModelKind::Spherical).unwrap();
        assert!(fitted.nugget >= 0.0);
        assert!(fitted.sill >= fitted.nugget);
    }

    #[test]
    fn every_refusal_is_named() {
        let (samples, values) = line();
        assert_eq!(empirical(&samples, &values, Some(4.0), 0), Err(Error::NoClasses));
        assert_eq!(
            empirical(&samples, &values[..3], None, 4),
            Err(Error::MismatchedLengths { count: 9, values: 3 })
        );
        assert_eq!(
            empirical::<2>(&[[0.0, 0.0]], &[1.0], None, 4),
            Err(Error::TooFewSamples { usable: 1, needed: 2 })
        );
        assert!(Model::new(ModelKind::Spherical, -0.1, 1.0, 5.0).is_err());
        assert!(Model::new(ModelKind::Spherical, 0.5, 0.1, 5.0).is_err());
        assert!(Model::new(ModelKind::Spherical, 0.0, 1.0, 0.0).is_err());
        let thin = Empirical {
            bins: vec![Bin {
                distance: 1.0,
                semivariance: 1.0,
                pairs: 4,
            }],
            set_aside: 0,
        };
        assert_eq!(
            Model::fit(&thin, ModelKind::Spherical),
            Err(Error::TooFewClasses { held: 1, needed: 3 })
        );
    }
}
