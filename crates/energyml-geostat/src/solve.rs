//! A dense LU factorisation with partial pivoting — the ~60 lines that
//! spare the crate a linear-algebra dependency, shared by kriging and RBF.
//!
//! Both systems are symmetric but **not** positive definite (the Lagrange
//! border of ordinary kriging, the polynomial border of a conditionally
//! positive definite RBF kernel), so pivoting is a requirement, not
//! caution — a Cholesky would fail on exactly the matrices these methods
//! build. Factor once, back-substitute per right-hand side.

use crate::Error;

pub(crate) struct Lu {
    side: usize,
    packed: Vec<f64>,
    pivots: Vec<usize>,
}

impl Lu {
    /// Factor a row-major `side × side` matrix. Refuses a singular one by
    /// name instead of dividing by a vanishing pivot.
    pub(crate) fn factor(mut packed: Vec<f64>, side: usize) -> Result<Lu, Error> {
        let mut pivots: Vec<usize> = (0..side).collect();
        for column in 0..side {
            let best = (column..side)
                .max_by(|a, b| {
                    packed[a * side + column].abs().total_cmp(&packed[b * side + column].abs())
                })
                .expect("a non-empty column");
            if packed[best * side + column].abs() < 1e-12 {
                return Err(Error::SingularSystem);
            }
            if best != column {
                for at in 0..side {
                    packed.swap(column * side + at, best * side + at);
                }
                pivots.swap(column, best);
            }
            let pivot = packed[column * side + column];
            for row in column + 1..side {
                let factor = packed[row * side + column] / pivot;
                packed[row * side + column] = factor;
                for at in column + 1..side {
                    packed[row * side + at] -= factor * packed[column * side + at];
                }
            }
        }
        Ok(Lu { side, packed, pivots })
    }

    pub(crate) fn solve(&self, rhs: &[f64]) -> Vec<f64> {
        let side = self.side;
        let mut x: Vec<f64> = self.pivots.iter().map(|&from| rhs[from]).collect();
        for row in 1..side {
            for column in 0..row {
                x[row] -= self.packed[row * side + column] * x[column];
            }
        }
        for row in (0..side).rev() {
            for column in row + 1..side {
                x[row] -= self.packed[row * side + column] * x[column];
            }
            x[row] /= self.packed[row * side + row];
        }
        x
    }
}
