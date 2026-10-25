// SPDX-License-Identifier: Apache-2.0
//! Eigenvalues and eigenvectors of a symmetric 3×3 matrix by cyclic Jacobi
//! rotations.
//!
//! Each sweep rotates the pairs `(0, 1)`, `(0, 2)` and `(1, 2)` in that
//! order, so equal inputs give equal bits. The sweeps stop when the
//! off-diagonal norm is below `JACOBI_RELATIVE_RESIDUAL` times the matrix
//! norm, or at `MAX_JACOBI_SWEEPS`. The off-diagonal norm left bounds the
//! eigenvalue error (Weyl).

use crate::tolerance::JACOBI_RELATIVE_RESIDUAL;

/// Jacobi sweeps of one eigen-decomposition.
const MAX_JACOBI_SWEEPS: usize = 32;

/// The eigen-decomposition of a symmetric 3×3 matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Eigen {
    /// Eigenvalues, ascending.
    pub(crate) values: [f64; 3],
    /// Unit eigenvectors in the order of `values`, each with its component
    /// of largest magnitude positive.
    pub(crate) vectors: [[f64; 3]; 3],
    /// The Frobenius norm of the off-diagonal part left by the last sweep.
    pub(crate) residual: f64,
}

fn off_diagonal(matrix: &[[f64; 3]; 3]) -> f64 {
    let [xy, zx, yz] = [matrix[0][1], matrix[0][2], matrix[1][2]];
    (2.0 * (xy * xy + zx * zx + yz * yz)).sqrt()
}

fn frobenius(matrix: &[[f64; 3]; 3]) -> f64 {
    matrix
        .iter()
        .flatten()
        .map(|entry| entry * entry)
        .sum::<f64>()
        .sqrt()
}

/// The rotation that zeroes `matrix[first][second]`, applied to `matrix`
/// and accumulated in the columns of `basis`.
fn rotate(matrix: &mut [[f64; 3]; 3], basis: &mut [[f64; 3]; 3], first: usize, second: usize) {
    let off = matrix[first][second];
    if off == 0.0 {
        return;
    }
    let theta = (matrix[second][second] - matrix[first][first]) / (2.0 * off);
    let tangent = theta.signum() / (theta.abs() + theta.hypot(1.0));
    let cosine = 1.0 / tangent.hypot(1.0);
    let sine = tangent * cosine;
    let turn = |row: &mut [f64; 3]| {
        let (along, across) = (row[first], row[second]);
        row[first] = cosine * along - sine * across;
        row[second] = sine * along + cosine * across;
    };
    matrix.iter_mut().for_each(turn);
    let (along, across) = (matrix[first], matrix[second]);
    matrix[first] = std::array::from_fn(|column| cosine * along[column] - sine * across[column]);
    matrix[second] = std::array::from_fn(|column| sine * along[column] + cosine * across[column]);
    matrix[first][second] = 0.0;
    matrix[second][first] = 0.0;
    basis.iter_mut().for_each(turn);
}

/// The eigen-decomposition of the symmetric matrix `matrix`.
pub(crate) fn symmetric_eigen(matrix: [[f64; 3]; 3]) -> Eigen {
    let mut reduced = matrix;
    let mut basis = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let norm = frobenius(&reduced);
    for _ in 0..MAX_JACOBI_SWEEPS {
        if off_diagonal(&reduced) <= JACOBI_RELATIVE_RESIDUAL * norm {
            break;
        }
        for (first, second) in [(0, 1), (0, 2), (1, 2)] {
            rotate(&mut reduced, &mut basis, first, second);
        }
    }
    let mut order = [0, 1, 2];
    order.sort_by(|left, right| reduced[*left][*left].total_cmp(&reduced[*right][*right]));
    let vectors = order.map(|column| {
        let vector = basis.map(|row| row[column]);
        let largest = vector
            .iter()
            .copied()
            .reduce(|kept, entry| {
                if entry.abs() > kept.abs() {
                    entry
                } else {
                    kept
                }
            })
            .unwrap_or(1.0);
        if largest < 0.0 {
            vector.map(|entry| -entry)
        } else {
            vector
        }
    });
    Eigen {
        values: order.map(|index| reduced[index][index]),
        vectors,
        residual: off_diagonal(&reduced),
    }
}

#[cfg(test)]
mod tests {
    use super::symmetric_eigen;

    /// Rounding of a decomposition of entries of order one.
    const DECOMPOSITION_ROUNDING: f64 = 1.0e-14;

    fn reconstruct(eigen: &super::Eigen) -> [[f64; 3]; 3] {
        std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                (0..3)
                    .map(|k| eigen.values[k] * eigen.vectors[k][i] * eigen.vectors[k][j])
                    .sum()
            })
        })
    }

    #[test]
    fn a_diagonal_matrix_sorts_its_axes_by_value() {
        let eigen = symmetric_eigen([[3.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 2.0]]);
        assert_eq!(eigen.values, [1.0, 2.0, 3.0]);
        assert_eq!(
            eigen.vectors,
            [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]]
        );
        assert_eq!(eigen.residual, 0.0);
    }

    /// The rotation by 30° about `z` of `diag(1, 4, 9)`: eigenvalues 1, 4
    /// and 9 with eigenvectors `(cos 30°, sin 30°, 0)`, `(−sin 30°, cos 30°,
    /// 0)` and `z`.
    #[test]
    fn a_rotated_diagonal_matrix_gives_its_values_and_axes() {
        let (s, c) = std::f64::consts::FRAC_PI_6.sin_cos();
        let matrix = [
            [c * c + 4.0 * s * s, -3.0 * c * s, 0.0],
            [-3.0 * c * s, s * s + 4.0 * c * c, 0.0],
            [0.0, 0.0, 9.0],
        ];
        let eigen = symmetric_eigen(matrix);
        for (value, expected) in eigen.values.iter().zip([1.0, 4.0, 9.0]) {
            assert!((value - expected).abs() <= DECOMPOSITION_ROUNDING * 10.0);
        }
        let expected = [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]];
        for (vector, expected) in eigen.vectors.iter().zip(expected) {
            let sign = if expected
                .iter()
                .copied()
                .reduce(|kept, entry| {
                    if entry.abs() > kept.abs() {
                        entry
                    } else {
                        kept
                    }
                })
                .unwrap()
                < 0.0
            {
                -1.0
            } else {
                1.0
            };
            for (entry, expected) in vector.iter().zip(expected) {
                assert!((entry - sign * expected).abs() <= DECOMPOSITION_ROUNDING * 10.0);
            }
        }
        let back = reconstruct(&eigen);
        for i in 0..3 {
            for j in 0..3 {
                assert!((back[i][j] - matrix[i][j]).abs() <= DECOMPOSITION_ROUNDING * 100.0);
            }
        }
        assert!(eigen.residual <= DECOMPOSITION_ROUNDING * 100.0);
    }

    #[test]
    fn a_full_matrix_reconstructs_and_equal_inputs_give_equal_bits() {
        let matrix = [[4.0, 1.0, -2.0], [1.0, 2.0, 0.5], [-2.0, 0.5, 3.0]];
        let eigen = symmetric_eigen(matrix);
        assert_eq!(eigen, symmetric_eigen(matrix));
        assert!(eigen.values[0] <= eigen.values[1] && eigen.values[1] <= eigen.values[2]);
        let trace: f64 = eigen.values.iter().sum();
        assert!((trace - 9.0).abs() <= DECOMPOSITION_ROUNDING * 100.0);
        let back = reconstruct(&eigen);
        for i in 0..3 {
            for j in 0..3 {
                assert!((back[i][j] - matrix[i][j]).abs() <= DECOMPOSITION_ROUNDING * 100.0);
            }
        }
        for (i, first) in eigen.vectors.iter().enumerate() {
            for (j, second) in eigen.vectors.iter().enumerate() {
                let dot: f64 = first.iter().zip(second).map(|(a, b)| a * b).sum();
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((dot - expected).abs() <= DECOMPOSITION_ROUNDING * 100.0);
            }
        }
    }
}
