// SPDX-License-Identifier: Apache-2.0
//! Homogeneous Bezier extraction and rational boundary bounds.

use crate::math::sum::ExactSignedSum;
use cadmpeg_core::decode::alloc_filled;

/// One nonempty knot span and its homogeneous Bezier control polygon.
#[derive(Clone, Debug)]
pub struct HomogeneousBezierSpan<const DIMENSION: usize = 4> {
    /// The span's original parameter interval.
    pub domain: [f64; 2],
    /// Weighted coordinates followed by their weight, such as `(w*x, w*y, w*z, w)`.
    pub controls: Vec<[f64; DIMENSION]>,
}

/// Form a positive-weight homogeneous polygon without common-scale overflow.
/// Refuse a relative weight or coordinate product that would disappear.
pub fn positive_controls(points: &[crate::math::Point3], weights: &[f64]) -> Option<Vec<[f64; 4]>> {
    if points.len() != weights.len()
        || points.is_empty()
        || weights
            .iter()
            .any(|weight| !weight.is_finite() || *weight <= 0.0)
    {
        return None;
    }
    let scale = weights.iter().copied().fold(0.0_f64, f64::max);
    points
        .iter()
        .zip(weights)
        .map(|(point, weight)| {
            let weight = weight / scale;
            if weight == 0.0 || !point.is_finite() {
                return None;
            }
            let result = [weight * point.x, weight * point.y, weight * point.z, weight];
            if result.iter().any(|v| !v.is_finite())
                || [point.x, point.y, point.z]
                    .iter()
                    .zip(result)
                    .any(|(raw, product)| *raw != 0.0 && product == 0.0)
            {
                return None;
            }
            Some(result)
        })
        .collect()
}

fn insert_knot<const DIMENSION: usize>(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; DIMENSION]>,
    value: f64,
) -> Option<()> {
    let last = controls.len().checked_sub(1)?;
    let span = knots.iter().rposition(|knot| *knot <= value)?;
    let multiplicity = knots.iter().filter(|knot| **knot == value).count();
    let first = span.checked_sub(degree)?;
    let tail = span.checked_sub(multiplicity)?;
    if multiplicity > degree || first > last || tail > last {
        return None;
    }
    let mut inserted = alloc_filled(
        controls.len().checked_add(1)?,
        [0.0; DIMENSION],
        "Bezier knot insertion",
    )
    .ok()?;
    inserted[..=first].copy_from_slice(&controls[..=first]);
    inserted[tail + 1..].copy_from_slice(&controls[tail..]);
    for index in first + 1..=tail {
        let alpha =
            crate::math::parameter_fraction(value, knots[index], knots[index + degree])?.get();
        inserted[index] = std::array::from_fn(|axis| {
            (1.0 - alpha) * controls[index - 1][axis] + alpha * controls[index][axis]
        });
    }
    knots.insert(span + 1, value);
    *controls = inserted;
    Some(())
}

/// Extract the active spans, including unclamped endpoints and discontinuities.
///
/// Knot multiplicity controls the control-point index of each span. A full
/// internal multiplicity starts a new polygon without sharing an endpoint.
pub fn homogeneous_spans<const DIMENSION: usize>(
    degree: usize,
    knots: &[f64],
    mut controls: Vec<[f64; DIMENSION]>,
) -> Option<Vec<HomogeneousBezierSpan<DIMENSION>>> {
    let count = controls.len();
    if degree >= count
        || knots.len() != count.checked_add(degree)?.checked_add(1)?
        || knots.iter().any(|value| !value.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
        || controls.iter().flatten().any(|value| !value.is_finite())
    {
        return None;
    }
    let domain = [knots[degree], knots[count]];
    if domain[0] >= domain[1] {
        return None;
    }
    let mut knots = knots.to_vec();
    for endpoint in domain {
        while knots.iter().filter(|knot| **knot == endpoint).count() < degree + 1 {
            insert_knot(degree, &mut knots, &mut controls, endpoint)?;
        }
    }
    let mut internal = knots
        .iter()
        .copied()
        .filter(|knot| domain[0] < *knot && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            insert_knot(degree, &mut knots, &mut controls, knot)?;
        }
    }
    let spans = (degree..controls.len())
        .filter_map(|span| {
            let interval = [knots[span], knots[span + 1]];
            (interval[0] < interval[1] && interval[0] >= domain[0] && interval[1] <= domain[1])
                .then(|| HomogeneousBezierSpan {
                    domain: interval,
                    controls: controls[span - degree..=span].to_vec(),
                })
        })
        .collect::<Vec<_>>();
    (!spans.is_empty()).then_some(spans)
}

/// Prove equal-degree positive-weight rational Bezier boundaries agree.
///
/// Bernstein cross-products are compared before converting their scale back
/// to `f64`, so a nonzero difference cannot disappear through underflow.
pub fn boundaries_within_resolution(
    first: &[[f64; 4]],
    second: &[[f64; 4]],
    resolution: f64,
) -> Option<bool> {
    if first.is_empty()
        || first.len() != second.len()
        || !resolution.is_finite()
        || resolution < 0.0
        || first
            .iter()
            .chain(second)
            .any(|point| point.iter().any(|v| !v.is_finite()) || point[3] <= 0.0)
    {
        return None;
    }
    let degree = first.len() - 1;
    let product_degree = degree.checked_mul(2)?;
    let binomial = |n: usize, k: usize| {
        let k = k.min(n - k);
        (1..=k).fold(1.0, |value, factor| {
            value * (n - k + factor) as f64 / factor as f64
        })
    };
    let first_weight = first
        .iter()
        .map(|control| control[3])
        .fold(f64::INFINITY, f64::min);
    let second_weight = second
        .iter()
        .map(|control| control[3])
        .fold(f64::INFINITY, f64::min);
    let mut threshold = ExactSignedSum::default();
    threshold.add_factors([
        resolution,
        first_weight,
        second_weight,
        1.0 / 3.0_f64.sqrt(),
    ]);
    let threshold = threshold.finish();
    for index in 0..=product_degree {
        let mut cross = [ExactSignedSum::default(); 3];
        // `index` runs to twice the degree, and each factor keeps its own degree.
        // The pair `(first_index, second_index)` contributes when the two indices
        // sum to `index` and both stay inside the control net: a larger
        // `first_index` than `index` has no partner, and a smaller one than
        // `index - degree` asks for a second index past the end.
        for (first_index, first_control) in first.iter().enumerate() {
            let Some(second_index) = index.checked_sub(first_index) else {
                break;
            };
            let Some(second_control) = second.get(second_index) else {
                continue;
            };
            let coefficient = binomial(degree, first_index) * binomial(degree, second_index)
                / binomial(product_degree, index);
            if !coefficient.is_finite() {
                return None;
            }
            for (axis, component) in cross.iter_mut().enumerate() {
                component.add_factors([coefficient, first_control[axis], second_control[3]]);
                component.add_factors([-coefficient, second_control[axis], first_control[3]]);
            }
        }
        for component in cross {
            if let Some(value) = component.finish() {
                let Some(limit) = threshold else {
                    return Some(false);
                };
                let exponent = value.exponent().max(limit.exponent());
                if value.rescale(exponent)?.abs() > limit.rescale(exponent)?.abs() {
                    return Some(false);
                }
            }
        }
    }
    Some(true)
}

#[cfg(test)]
mod tests {
    use super::{boundaries_within_resolution, homogeneous_spans};
    #[test]
    fn numerical_followup_bezier_spans_preserve_unclamped_and_discontinuous_curves() {
        let spans = homogeneous_spans(
            2,
            &[-1., -1., 0., 1., 2., 2.],
            vec![[0., 0., 0., 1.], [1., 1., 0., 1.], [2., 0., 0., 1.]],
        )
        .unwrap();
        let controls = &spans[0].controls;
        let midpoint = (controls[0][1] + 2.0 * controls[1][1] + controls[2][1]) / 4.0;
        assert_eq!(midpoint, 0.75);
        let spans = homogeneous_spans(
            2,
            &[0., 0., 0., 1., 1., 1., 2., 2., 2.],
            (0..6).map(|i| [f64::from(i), 0., 0., 1.]).collect(),
        )
        .unwrap();
        assert_eq!(
            spans[1].controls.iter().map(|p| p[0]).collect::<Vec<_>>(),
            [3., 4., 5.]
        );
        let zero =
            homogeneous_spans(0, &[0., 1., 2.], vec![[0., 0., 0., 1.], [1., 0., 0., 1.]]).unwrap();
        assert_eq!(zero.len(), 2);
    }
    #[test]
    fn numerical_followup_boundary_certificate_keeps_small_cross_products() {
        for w in [1.0, 1e-200, 1e200] {
            let a = [[0., 0., 0., w], [w, 0., 0., w]];
            let b = [[0., 2.0 * w, 0., w], [w, 2.0 * w, 0., w]];
            assert_eq!(boundaries_within_resolution(&a, &b, 0.001), Some(false));
            assert_eq!(boundaries_within_resolution(&a, &a, 0.0), Some(true));
        }
    }

    #[test]
    fn numerical_0922b_bezier_insertion_preserves_wide_knot_units() {
        let controls = vec![
            [0., 0., 0., 1.],
            [1., 0., 0., 1.],
            [1., 1., 0., 1.],
            [2., 1., 0., 1.],
        ];
        let expected =
            homogeneous_spans(2, &[-1., -1., -1., 0., 1., 1., 1.], controls.clone()).unwrap();
        let actual = homogeneous_spans(
            2,
            &[-1e308, -1e308, -1e308, 0., 1e308, 1e308, 1e308],
            controls,
        )
        .unwrap();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual.controls, expected.controls);
            assert_eq!(actual.domain.map(|t| t / 1e308), expected.domain);
        }
    }
}
