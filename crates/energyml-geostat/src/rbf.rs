//! Radial basis function interpolation — the smooth surface through
//! scattered data, where kriging is the statistical one.
//!
//! `s(x) = Σ λᵢ φ(|x − xᵢ|) + p(x)`: one coefficient per sample plus a
//! low-degree polynomial, solved as one dense system. The polynomial is
//! not decoration — it is what makes the system solvable at all for the
//! kernels that are only **conditionally** positive definite, and its
//! degree is a property of the kernel, not a knob:
//!
//! | kernel | φ(r) | polynomial required |
//! |---|---|---|
//! | thin-plate spline | r² ln r | affine (order-2 CPD) |
//! | multiquadric | √(r² + ε²) | constant (order-1 CPD) |
//! | linear | r | constant (order-1 CPD) |
//! | gaussian | e^(−(εr)²) | none (positive definite) |
//!
//! Getting that column count wrong does not fail — it produces a smooth,
//! plausible, wrong surface, which is why the degree is decided by
//! [`Kernel`] and not exposed. One consequence is named in the errors: the
//! thin-plate's affine term needs samples that span the space, so **all
//! samples on one line** (in 2D) make the system singular — the refusal
//! says so rather than leaving `SingularSystem` to be puzzled out.
//!
//! Same contract as the crate's other interpolators: exact at samples, a
//! `NaN` sample set aside and counted, coincident samples refused with
//! both indices, factor once and back-substitute per query.
//!
//! TODO(reference): verify — radial basis function interpolation with conditionally positive definite kernels follows the standard literature; no publication is cited in this code. Add a reference once checked.

use crate::Error;
use crate::solve::Lu;

/// The radial kernel — which smoothness the surface buys.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kernel {
    /// The classic surface interpolator: minimises bending energy. No
    /// shape parameter to tune, which is a feature.
    ThinPlate,
    /// `√(r² + ε²)` — `epsilon` sets how rounded the surface is near the
    /// samples; the data's typical spacing is a reasonable value.
    Multiquadric { epsilon: f64 },
    /// `r` — a cone at each sample: continuous, not smooth, never
    /// overshoots wildly.
    Linear,
    /// `e^(−(εr)²)` — very smooth and very local; ill-conditioned when
    /// `epsilon` is small relative to the spacing.
    Gaussian { epsilon: f64 },
}

impl Kernel {
    fn evaluate(&self, r: f64) -> f64 {
        match *self {
            Kernel::ThinPlate => {
                if r <= 0.0 {
                    0.0
                } else {
                    r * r * r.ln()
                }
            }
            Kernel::Multiquadric { epsilon } => (r * r + epsilon * epsilon).sqrt(),
            Kernel::Linear => r,
            Kernel::Gaussian { epsilon } => (-(epsilon * r) * (epsilon * r)).exp(),
        }
    }

    /// Polynomial columns beyond the λ's: 0, 1 (constant) or 1 + N (affine).
    fn polynomial_terms(&self, dimensions: usize) -> usize {
        match self {
            Kernel::ThinPlate => 1 + dimensions,
            Kernel::Multiquadric { .. } | Kernel::Linear => 1,
            Kernel::Gaussian { .. } => 0,
        }
    }

    fn check(&self) -> Result<(), Error> {
        match *self {
            Kernel::Multiquadric { epsilon } | Kernel::Gaussian { epsilon } => {
                if !(epsilon.is_finite() && epsilon > 0.0) {
                    return Err(Error::BadModel {
                        what: format!("epsilon {epsilon}"),
                    });
                }
            }
            Kernel::ThinPlate | Kernel::Linear => {}
        }
        Ok(())
    }
}

/// RBF interpolation over a kernel.
#[derive(Debug, Clone)]
pub struct Rbf {
    pub kernel: Kernel,
    /// A query closer than this to a sample *is* that sample — same rule
    /// and same default as [`crate::Idw`].
    pub exact_within: f64,
}

/// What an RBF run answers.
#[derive(Debug, Clone, PartialEq)]
pub struct RbfAnswer {
    /// One value per query, in order.
    pub values: Vec<f64>,
    /// Samples set aside for a `NaN` value or coordinate.
    pub set_aside: usize,
}

impl Rbf {
    pub fn new(kernel: Kernel) -> Self {
        Rbf {
            kernel,
            exact_within: 1e-12,
        }
    }

    /// Interpolate `values` known at `samples` onto `queries`.
    pub fn interpolate<const N: usize>(
        &self,
        samples: &[[f64; N]],
        values: &[f64],
        queries: &[[f64; N]],
    ) -> Result<RbfAnswer, Error> {
        self.kernel.check()?;
        if samples.len() != values.len() {
            return Err(Error::MismatchedLengths {
                count: samples.len(),
                values: values.len(),
            });
        }
        let usable: Vec<([f64; N], f64)> = samples
            .iter()
            .zip(values)
            .filter(|(point, value)| value.is_finite() && point.iter().all(|c| c.is_finite()))
            .map(|(point, value)| (*point, *value))
            .collect();
        let set_aside = samples.len() - usable.len();
        let terms = self.kernel.polynomial_terms(N);
        if usable.len() < terms.max(1) {
            return Err(Error::TooFewSamples {
                usable: usable.len(),
                needed: terms.max(1),
            });
        }

        let distance = |a: &[f64; N], b: &[f64; N]| -> f64 {
            a.iter()
                .zip(b)
                .map(|(x, y)| (x - y) * (x - y))
                .sum::<f64>()
                .sqrt()
        };
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

        // [Φ P; Pᵀ 0] [λ; c] = [z; 0], with P the polynomial columns.
        let n = usable.len();
        let side = n + terms;
        let mut matrix = vec![0.0f64; side * side];
        for i in 0..n {
            for j in 0..n {
                matrix[i * side + j] = self.kernel.evaluate(distance(&usable[i].0, &usable[j].0));
            }
            if terms > 0 {
                matrix[i * side + n] = 1.0;
                matrix[n * side + i] = 1.0;
                for axis in 0..terms.saturating_sub(1) {
                    matrix[i * side + n + 1 + axis] = usable[i].0[axis];
                    matrix[(n + 1 + axis) * side + i] = usable[i].0[axis];
                }
            }
        }
        let factored = Lu::factor(matrix, side).map_err(|refusal| {
            // The one degenerate geometry a caller can actually hit: the
            // affine term of the thin-plate needs samples spanning the
            // space, and a survey along one seismic line does not.
            if terms > 1 {
                Error::DegenerateGeometry
            } else {
                refusal
            }
        })?;
        let mut rhs = vec![0.0f64; side];
        rhs[..n].copy_from_slice(&usable.iter().map(|(_, value)| *value).collect::<Vec<f64>>());
        let coefficients = factored.solve(&rhs);

        let mut out = Vec::with_capacity(queries.len());
        for query in queries {
            if let Some((_, value)) = usable
                .iter()
                .find(|(point, _)| distance(point, query) <= self.exact_within)
            {
                out.push(*value);
                continue;
            }
            let mut value: f64 = usable
                .iter()
                .zip(&coefficients[..n])
                .map(|((point, _), lambda)| lambda * self.kernel.evaluate(distance(point, query)))
                .sum();
            if terms > 0 {
                value += coefficients[n];
                for axis in 0..terms.saturating_sub(1) {
                    value += coefficients[n + 1 + axis] * query[axis];
                }
            }
            out.push(value);
        }
        Ok(RbfAnswer {
            values: out,
            set_aside,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KERNELS: [Kernel; 4] = [
        Kernel::ThinPlate,
        Kernel::Multiquadric { epsilon: 1.0 },
        Kernel::Linear,
        Kernel::Gaussian { epsilon: 0.5 },
    ];

    #[test]
    fn every_kernel_is_exact_at_its_samples() {
        let samples = [[0.0, 0.0], [4.0, 0.0], [2.0, 3.0], [1.0, 1.0]];
        let values = [1.0, 2.0, 3.0, -1.0];
        for kernel in KERNELS {
            let answer = Rbf::new(kernel).interpolate(&samples, &values, &samples).unwrap();
            for (got, expected) in answer.values.iter().zip(&values) {
                assert!((got - expected).abs() < 1e-8, "{kernel:?}: {got} vs {expected}");
            }
        }
    }

    #[test]
    fn kernels_with_a_polynomial_reproduce_what_the_polynomial_spans() {
        // Thin-plate carries an affine term: an affine field comes back
        // exactly, everywhere — the λ's solve to zero and the polynomial
        // does the whole job. Constant-term kernels do the same for a
        // constant field.
        let samples = [[0.0, 0.0], [4.0, 0.0], [2.0, 3.0], [1.0, 1.0]];
        let affine = |p: &[f64; 2]| 2.0 + 3.0 * p[0] - 0.5 * p[1];
        let values: Vec<f64> = samples.iter().map(affine).collect();
        let queries = [[10.0, 10.0], [-3.0, 7.0]];
        let answer = Rbf::new(Kernel::ThinPlate)
            .interpolate(&samples, &values, &queries)
            .unwrap();
        for (got, query) in answer.values.iter().zip(&queries) {
            assert!((got - affine(query)).abs() < 1e-8, "{got} vs {}", affine(query));
        }

        let flat = [7.0; 4];
        for kernel in [Kernel::Multiquadric { epsilon: 1.0 }, Kernel::Linear] {
            let answer = Rbf::new(kernel).interpolate(&samples, &flat, &queries).unwrap();
            for got in &answer.values {
                assert!((got - 7.0).abs() < 1e-8, "{kernel:?}: {got}");
            }
        }
    }

    #[test]
    fn the_midpoint_of_two_samples_is_their_mean_for_the_symmetric_kernels() {
        for kernel in [Kernel::Multiquadric { epsilon: 1.0 }, Kernel::Linear] {
            let answer = Rbf::new(kernel)
                .interpolate(&[[0.0, 0.0], [2.0, 0.0]], &[1.0, 3.0], &[[1.0, 0.0]])
                .unwrap();
            assert!((answer.values[0] - 2.0).abs() < 1e-9, "{kernel:?}");
        }
    }

    #[test]
    fn a_nan_sample_is_set_aside_and_counted() {
        let samples = [[0.0, 0.0], [2.0, 0.0], [1.0, 4.0], [3.0, 1.0]];
        let values = [1.0, 3.0, f64::NAN, 2.0];
        let answer = Rbf::new(Kernel::Linear)
            .interpolate(&samples, &values, &[[0.0, 0.0]])
            .unwrap();
        assert_eq!(answer.set_aside, 1);
        assert_eq!(answer.values, vec![1.0]);
    }

    #[test]
    fn collinear_samples_refuse_the_thin_plate_by_name() {
        // A survey along one line: the affine term cannot be determined
        // across it. The refusal says the geometry is the problem.
        let samples = [[0.0, 0.0], [1.0, 0.0], [2.0, 0.0], [3.0, 0.0]];
        let values = [1.0, 2.0, 3.0, 4.0];
        let refusal = Rbf::new(Kernel::ThinPlate).interpolate(&samples, &values, &[[1.0, 1.0]]);
        assert_eq!(refusal, Err(Error::DegenerateGeometry));
        // The constant-term kernels do not care.
        assert!(
            Rbf::new(Kernel::Linear)
                .interpolate(&samples, &values, &[[1.0, 1.0]])
                .is_ok()
        );
    }

    #[test]
    fn three_dimensions_work_the_same_way() {
        let samples = [[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]];
        let values = [1.0, 3.0, 5.0, 7.0];
        let answer = Rbf::new(Kernel::Linear)
            .interpolate(&samples, &values, &samples)
            .unwrap();
        assert_eq!(answer.values, values.to_vec());
    }

    #[test]
    fn every_refusal_is_named() {
        let s = [[0.0, 0.0], [1.0, 0.0]];
        assert_eq!(
            Rbf::new(Kernel::Multiquadric { epsilon: 0.0 }).interpolate(&s, &[1.0, 2.0], &[]),
            Err(Error::BadModel {
                what: "epsilon 0".into()
            })
        );
        assert_eq!(
            Rbf::new(Kernel::Linear).interpolate(&s, &[1.0], &[]),
            Err(Error::MismatchedLengths { count: 2, values: 1 })
        );
        assert_eq!(
            Rbf::new(Kernel::ThinPlate).interpolate(&s, &[1.0, 2.0], &[]),
            Err(Error::TooFewSamples { usable: 2, needed: 3 })
        );
        assert_eq!(
            Rbf::new(Kernel::Linear).interpolate(&[[0.0, 0.0], [0.0, 0.0]], &[1.0, 2.0], &[[1.0, 0.0]]),
            Err(Error::DuplicateSamplePositions { first: 0, second: 1 })
        );
    }
}
