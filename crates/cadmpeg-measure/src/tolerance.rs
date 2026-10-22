// SPDX-License-Identifier: Apache-2.0
//! Named numeric tolerances and the per-face allowance and model tolerance.
//!
//! Each constant has one purpose. Two purposes that share a value keep two
//! constants.

use cadmpeg_ir::units::COINCIDENCE_TOLERANCE;

/// Outer quadrature target relative to `max(|value|, scale^d)`.
pub(crate) const QUADRATURE_RELATIVE_TARGET: f64 = 1.0e-10;

/// Above this estimate relative to `scale^d` the value is
/// `quadrature.not-converged`.
pub(crate) const QUADRATURE_ACCEPTANCE_RELATIVE: f64 = 1.0e-6;

/// Root refinement width relative to the searched interval.
pub(crate) const EXTREMUM_PARAMETER_RESOLUTION: f64 = 1.0e-12;

/// Generic inner area integral target relative to the inner integral's
/// own magnitude.
pub(crate) const INNER_QUADRATURE_RELATIVE_TARGET: f64 = 1.0e-12;

/// Newton refinement of an inverted surface point stops when its step is
/// below this fraction of the domain span.
pub(crate) const INVERSION_PARAMETER_RESOLUTION: f64 = 1.0e-12;

/// Gram-matrix test for similarity and rigid placements. An admitted unit
/// frame deviates from orthonormal by up to about `2e-9` per Gram entry, so
/// this admits a composition of up to four admitted frames.
pub(crate) const SIMILARITY_RELATIVE_TOLERANCE: f64 = 1.0e-8;

/// Polyline deviation for winding numbers and the membership walk, relative
/// to the loop's parameter-space box diagonal.
pub(crate) const NESTING_SAMPLE_RELATIVE_DEVIATION: f64 = 1.0e-3;

/// A ring loop that encloses less than this fraction of its 3D box diagonal
/// squared encloses no area.
pub(crate) const LOOP_DEGENERATE_RELATIVE_AREA: f64 = 1.0e-12;

/// Largest of the inputs, ignoring absent ones.
fn largest(values: impl IntoIterator<Item = f64>) -> f64 {
    values.into_iter().fold(0.0, f64::max)
}

/// The acceptance allowance α of a face: the coincidence tolerance, the
/// document linear tolerance, and the face, edge and vertex tolerances. α is
/// an acceptance gate only; it does not enter model uncertainty.
pub(crate) fn allowance(linear: f64, local: impl IntoIterator<Item = f64>) -> f64 {
    largest([COINCIDENCE_TOLERANCE, linear].into_iter().chain(local))
}

/// The model tolerance τ of a face: the document linear tolerance and the
/// face, edge, vertex and fit tolerances. τ enters model uncertainty.
pub(crate) fn model_tolerance(linear: f64, local: impl IntoIterator<Item = f64>) -> f64 {
    largest(std::iter::once(linear).chain(local))
}

#[cfg(test)]
mod tests {
    #[test]
    fn allowance_and_model_tolerance_take_the_largest_stated_tolerance() {
        let linear = 1.0e-7;
        assert_eq!(
            super::allowance(linear, []),
            cadmpeg_ir::units::COINCIDENCE_TOLERANCE
        );
        assert_eq!(super::allowance(linear, [0.5]), 0.5);
        assert_eq!(super::model_tolerance(linear, []), linear);
        assert_eq!(super::model_tolerance(linear, [2.0e-6, 1.0e-6]), 2.0e-6);
    }
}
