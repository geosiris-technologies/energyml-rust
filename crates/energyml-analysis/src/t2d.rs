//! Analytic time-to-depth — the vertical change of variable a seismic
//! interpretation needs before it can meet a well.
//!
//! The law is `V(z) = V₀ + k·z`: instantaneous velocity growing linearly
//! with depth below the datum, the classic single-parameter-per-layer
//! model. One-way time integrates to `τ(z) = ln(1 + k·z/V₀) / k`, which
//! inverts to `z(τ) = V₀ · (e^{k·τ} − 1) / k`; `k = 0` is the constant
//! velocity `z = V₀·τ` and is a legitimate law, not an error.
//!
//! Two conventions are **in the names, not defaulted**, because both have
//! silently-wrong readings (the same lesson as the tube's radius-never-
//! diameter):
//! - Seismic times are usually **two-way**; every method says `owt` or
//!   `twt` and there is no unqualified `time`. Feeding a TWT to a OWT
//!   formula doubles every depth with no symptom.
//! - Depth counts **from the datum where t = 0**, positive downward, and
//!   `V₀` is the velocity *at that datum*. A caller whose CRS puts z
//!   elsewhere shifts before and after — this module does not guess.
//!
//! How this meets the model is an architecture decision: a **vertical transformation per object**, like
//! the `Vertical` of geometry's `parametric.rs` — and never inside
//! `Crs::to_projected`, whose placement runs once over everything and
//! would convert twice. The law lives here; wiring it to an object whose
//! `verticalIsTime` says so is the caller's seam.
//!
//! A `NaN` time yields a `NaN` depth: an undefined node is data, not a
//! malformed call, and it must come out as undefined — not as zero.

use crate::Error;

/// A velocity law in depth, `V(z) = V₀ + k·z` below the datum.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearVelocity {
    /// The velocity at the datum (z = 0, t = 0), in length units per
    /// second — the same length unit the depths come out in.
    pub v0: f64,
    /// The gradient, in 1/second. 0 is constant velocity.
    pub k: f64,
}

impl LinearVelocity {
    /// Refused by name when the numbers cannot be a velocity law: `V₀`
    /// must be a finite positive number, `k` finite.
    pub fn new(v0: f64, k: f64) -> Result<LinearVelocity, Error> {
        if !(v0.is_finite() && v0 > 0.0) {
            return Err(Error::BadModel { what: format!("v0 {v0} (a velocity is > 0)") });
        }
        if !k.is_finite() {
            return Err(Error::BadModel { what: format!("k {k}") });
        }
        Ok(LinearVelocity { v0, k })
    }

    /// Depth below the datum at one-way time `owt` (seconds).
    pub fn depth_of_owt(&self, owt: f64) -> f64 {
        if self.k == 0.0 {
            self.v0 * owt
        } else {
            // exp_m1 keeps small k·τ exact where e^x − 1 would cancel.
            self.v0 * (self.k * owt).exp_m1() / self.k
        }
    }

    /// Depth below the datum at two-way time `twt` (seconds).
    pub fn depth_of_twt(&self, twt: f64) -> f64 {
        self.depth_of_owt(twt / 2.0)
    }

    /// One-way time to reach depth `z` below the datum.
    ///
    /// `NaN` where the law does not reach: with `k < 0` the velocity hits
    /// zero at `z = −V₀/k` and no ray goes deeper — an undefined answer,
    /// not a plausible number.
    pub fn owt_of_depth(&self, z: f64) -> f64 {
        if self.k == 0.0 {
            z / self.v0
        } else {
            let ratio = self.k * z / self.v0;
            if ratio <= -1.0 {
                return f64::NAN;
            }
            ratio.ln_1p() / self.k
        }
    }

    /// Two-way time to reach depth `z` below the datum.
    pub fn twt_of_depth(&self, z: f64) -> f64 {
        2.0 * self.owt_of_depth(z)
    }

    /// The array form of [`Self::depth_of_twt`] — one depth per time, a
    /// `NaN` time staying a `NaN` depth.
    pub fn depths_of_twt(&self, twt: &[f64]) -> Vec<f64> {
        twt.iter().map(|&t| self.depth_of_twt(t)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_velocity_is_the_textbook_product() {
        let law = LinearVelocity::new(2000.0, 0.0).unwrap();
        // 2 s TWT at 2000 m/s: 1 s one way, 2000 m down.
        assert_eq!(law.depth_of_twt(2.0), 2000.0);
        assert_eq!(law.twt_of_depth(2000.0), 2.0);
    }

    #[test]
    fn the_two_directions_are_inverses() {
        let law = LinearVelocity::new(1800.0, 0.6).unwrap();
        for z in [0.0, 100.0, 1500.0, 4000.0] {
            let back = law.depth_of_owt(law.owt_of_depth(z));
            assert!((back - z).abs() < 1e-9 * z.max(1.0), "{z} came back as {back}");
        }
    }

    #[test]
    fn a_vanishing_gradient_meets_the_constant_law() {
        // Continuity at k → 0 is what exp_m1/ln_1p buy: the two branches
        // must agree to machine precision near the seam.
        let constant = LinearVelocity::new(2000.0, 0.0).unwrap();
        let nearly = LinearVelocity::new(2000.0, 1e-12).unwrap();
        for t in [0.1, 1.0, 3.0] {
            let a = constant.depth_of_twt(t);
            let b = nearly.depth_of_twt(t);
            assert!((a - b).abs() < 1e-6, "k=0 gives {a}, k=1e-12 gives {b}");
        }
    }

    #[test]
    fn a_positive_gradient_deepens_faster_than_its_datum_velocity() {
        let law = LinearVelocity::new(2000.0, 0.5).unwrap();
        assert!(law.depth_of_twt(2.0) > 2000.0, "the ray accelerates below the datum");
        // And time-to-depth stays monotonic — a fold here would let one
        // time mean two depths.
        let depths = law.depths_of_twt(&[0.5, 1.0, 1.5, 2.0]);
        assert!(depths.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn twt_is_twice_owt_never_a_silent_convention() {
        let law = LinearVelocity::new(2000.0, 0.3).unwrap();
        assert_eq!(law.depth_of_twt(2.0), law.depth_of_owt(1.0));
    }

    #[test]
    fn nan_stays_nan_and_the_unreachable_depth_says_so() {
        let law = LinearVelocity::new(2000.0, -0.5).unwrap();
        assert!(law.depth_of_twt(f64::NAN).is_nan(), "an undefined node stays undefined");
        // With k = −0.5, V hits zero at z = 4000: no ray reaches below.
        assert!(law.owt_of_depth(5000.0).is_nan());
        assert!(law.owt_of_depth(3000.0).is_finite());
    }

    #[test]
    fn every_refusal_is_named() {
        assert!(LinearVelocity::new(0.0, 0.5).is_err());
        assert!(LinearVelocity::new(-100.0, 0.5).is_err());
        assert!(LinearVelocity::new(f64::NAN, 0.5).is_err());
        assert!(LinearVelocity::new(2000.0, f64::INFINITY).is_err());
    }
}
