// SPDX-License-Identifier: Apache-2.0
//! Homogeneous Bezier extraction and rational boundary bounds.

use super::scoped::ScopedRows;
use super::PoleValue;
use crate::features::FinitePoint3;
use crate::math::sum::ExactSignedSum;
use cadmpeg_core::decode::{DecodeContext, ResourceLimit};

/// One nonempty knot span and its homogeneous Bezier control polygon.
#[derive(Clone, Debug)]
pub struct HomogeneousBezierSpan<const DIMENSION: usize = 4> {
    /// The span's original parameter interval.
    pub domain: [f64; 2],
    /// Weighted coordinates followed by their weight, such as `(w*x, w*y, w*z, w)`.
    pub controls: Vec<[f64; DIMENSION]>,
}

/// Form a positive-weight homogeneous polygon without common-scale overflow.
/// Refuse a raw pole with a non-finite coordinate, and a relative weight or
/// coordinate product that would disappear. Absent weights read as 1.0.
pub fn positive_controls<'ctx, P: PoleValue<FinitePoint3>>(
    _ctx: &'ctx DecodeContext<'_>,
    points: &[P],
    weights: Option<&[f64]>,
    _operation: &'static str,
) -> Result<Option<ScopedRows<'ctx, [f64; 4]>>, ResourceLimit> {
    if weights.is_some_and(|weights| points.len() != weights.len()) || points.is_empty() {
        return Ok(None);
    }
    let mut scale = 1.0;
    if let Some(weights) = weights {
        for weight in weights {
            if !weight.is_finite() || *weight <= 0.0 {
                return Ok(None);
            }
        }
        scale = 0.0_f64;
        for weight in weights {
            scale = scale.max(*weight);
        }
    }
    let mut output = Vec::new();
    for (index, point) in points.iter().enumerate() {
        let weight = weights.map_or(1.0, |weights| weights[index]) / scale;
        let Some(point) = point.admit() else {
            return Ok(None);
        };
        let point = point.get();
        if weight == 0.0 {
            return Ok(None);
        }
        let result = [weight * point.x, weight * point.y, weight * point.z, weight];
        if result.iter().any(|value| !value.is_finite())
            || [point.x, point.y, point.z]
                .iter()
                .zip(result)
                .any(|(raw, product)| *raw != 0.0 && product == 0.0)
        {
            return Ok(None);
        }
        output.push(result);
    }
    Ok(Some(ScopedRows::new(output)))
}

struct BezierWorkingSpline<const DIMENSION: usize> {
    knots: Vec<f64>,
    controls: Vec<[f64; DIMENSION]>,
}

fn knot_multiplicity(knots: &[f64], value: f64) -> usize {
    knots.iter().filter(|knot| **knot == value).count()
}

fn insert_knot<const DIMENSION: usize>(
    degree: usize,
    working: &mut BezierWorkingSpline<DIMENSION>,
    value: f64,
) -> Option<()> {
    let last = working.controls.len().checked_sub(1)?;
    let mut span = None;
    for (index, knot) in working.knots.iter().enumerate().rev() {
        if *knot <= value {
            span = Some(index);
            break;
        }
    }
    let span = span?;
    let multiplicity = knot_multiplicity(&working.knots, value);
    let first = span.checked_sub(degree)?;
    let tail = span.checked_sub(multiplicity)?;
    if multiplicity > degree || first > last || tail > last {
        return None;
    }
    working.controls.push([0.0; DIMENSION]);
    working.controls.copy_within(tail..=last, tail + 1);
    for index in (first + 1..=tail).rev() {
        let alpha = crate::math::parameter_fraction(
            value,
            working.knots[index],
            working.knots[index + degree],
        )
        .map(crate::scalar::FiniteReal::get)?;
        working.controls[index] = std::array::from_fn(|axis| {
            (1.0 - alpha) * working.controls[index - 1][axis]
                + alpha * working.controls[index][axis]
        });
    }

    working.knots.insert(span + 1, value);
    Some(())
}

/// Extract the active spans, including unclamped endpoints and discontinuities.
/// Knot multiplicity controls the control index of each span. Full internal
/// multiplicity starts a new polygon without a shared endpoint.
pub fn homogeneous_spans<'ctx, const DIMENSION: usize>(
    _ctx: &'ctx DecodeContext<'_>,
    degree: usize,
    knots: &[f64],
    controls: &[[f64; DIMENSION]],
) -> Result<Option<ScopedRows<'ctx, HomogeneousBezierSpan<DIMENSION>>>, ResourceLimit> {
    let count = controls.len();
    let Some(expected_knots) = count
        .checked_add(degree)
        .and_then(|value| value.checked_add(1))
    else {
        return Ok(None);
    };
    if degree >= count || knots.len() != expected_knots {
        return Ok(None);
    }
    for knot in knots {
        if !knot.is_finite() {
            return Ok(None);
        }
    }
    for pair in knots.windows(2) {
        if pair[0] > pair[1] {
            return Ok(None);
        }
    }
    for value in controls.iter().flatten() {
        if !value.is_finite() {
            return Ok(None);
        }
    }
    let domain = [knots[degree], knots[count]];
    if domain[0] >= domain[1] {
        return Ok(None);
    }
    let mut working = BezierWorkingSpline {
        knots: knots.to_vec(),
        controls: controls.to_vec(),
    };
    for endpoint in domain {
        while knot_multiplicity(&working.knots, endpoint) < degree + 1 {
            if insert_knot(degree, &mut working, endpoint).is_none() {
                return Ok(None);
            }
        }
    }
    let mut internal = Vec::new();
    for knot in &working.knots {
        if domain[0] < *knot && *knot < domain[1] {
            internal.push(*knot);
        }
    }
    if !internal.is_empty() {
        internal.dedup();
    }
    for knot in &internal {
        while knot_multiplicity(&working.knots, *knot) < degree {
            if insert_knot(degree, &mut working, *knot).is_none() {
                return Ok(None);
            }
        }
    }
    let mut spans = Vec::new();
    for span in degree..working.controls.len() {
        let interval = [working.knots[span], working.knots[span + 1]];
        if interval[0] < interval[1] && interval[0] >= domain[0] && interval[1] <= domain[1] {
            spans.push(HomogeneousBezierSpan {
                domain: interval,
                controls: working.controls[span - degree..=span].to_vec(),
            });
        }
    }
    if spans.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ScopedRows::new(spans)))
    }
}

/// Prove equal-degree positive-weight rational Bezier boundaries agree.
///
/// Bernstein cross-products are compared before converting their scale back
/// to `f64`, so a nonzero difference cannot disappear through underflow.
pub fn boundaries_within_resolution(
    _ctx: &DecodeContext<'_>,
    first: &[[f64; 4]],
    second: &[[f64; 4]],
    resolution: f64,
) -> Result<Option<bool>, ResourceLimit> {
    macro_rules! value {
        ($expression:expr) => {
            match $expression {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    if first.is_empty()
        || first.len() != second.len()
        || !resolution.is_finite()
        || resolution < 0.0
    {
        return Ok(None);
    }
    for control in first.iter().chain(second) {
        if control.iter().any(|value| !value.is_finite()) || control[3] <= 0.0 {
            return Ok(None);
        }
    }
    let degree = first.len() - 1;
    let product_degree = value!(degree.checked_mul(2));
    let binomial = |n: usize, k: usize| -> Result<Option<f64>, ResourceLimit> {
        let k = k.min(n - k);

        Ok((1..=k).try_fold(1.0, |value, factor| {
            Some(
                value * cadmpeg_core::convert::f64_from_index(n - k + factor)?
                    / cadmpeg_core::convert::f64_from_index(factor)?,
            )
        }))
    };
    let mut first_weight = f64::INFINITY;
    for control in first {
        first_weight = first_weight.min(control[3]);
    }
    let mut second_weight = f64::INFINITY;
    for control in second {
        second_weight = second_weight.min(control[3]);
    }
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
        // A control pair contributes when its indices sum to the product index.
        for (first_index, first_control) in first.iter().enumerate() {
            let Some(second_index) = index.checked_sub(first_index) else {
                break;
            };
            let Some(second_control) = second.get(second_index) else {
                continue;
            };
            let coefficient = value!(binomial(degree, first_index)?)
                * value!(binomial(degree, second_index)?)
                / value!(binomial(product_degree, index)?);
            if !coefficient.is_finite() {
                return Ok(None);
            }
            for (axis, component) in cross.iter_mut().enumerate() {
                component.add_factors([coefficient, first_control[axis], second_control[3]]);
                component.add_factors([-coefficient, second_control[axis], first_control[3]]);
            }
        }
        for component in cross {
            if let Some(value) = component.finish() {
                let Some(limit) = threshold else {
                    return Ok(Some(false));
                };
                let exponent = value.exponent().max(limit.exponent());
                if value!(value.rescale(exponent)).abs() > value!(limit.rescale(exponent)).abs() {
                    return Ok(Some(false));
                }
            }
        }
    }
    Ok(Some(true))
}

#[cfg(test)]
mod tests {
    use super::{boundaries_within_resolution, homogeneous_spans};

    #[test]
    fn numerical_followup_bezier_spans_preserve_unclamped_and_discontinuous_curves() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let spans = homogeneous_spans(
            &ctx,
            2,
            &[-1., -1., 0., 1., 2., 2.],
            &[[0., 0., 0., 1.], [1., 1., 0., 1.], [2., 0., 0., 1.]],
        )
        .expect("resource allocation did not fail")
        .unwrap();
        let controls = &spans[0].controls;
        let midpoint = (controls[0][1] + 2.0 * controls[1][1] + controls[2][1]) / 4.0;
        assert_eq!(midpoint, 0.75);
        let spans = homogeneous_spans(
            &ctx,
            2,
            &[0., 0., 0., 1., 1., 1., 2., 2., 2.],
            &(0..6)
                .map(|i| [f64::from(i), 0., 0., 1.])
                .collect::<Vec<_>>(),
        )
        .expect("resource allocation did not fail")
        .unwrap();
        assert_eq!(
            spans[1].controls.iter().map(|p| p[0]).collect::<Vec<_>>(),
            [3., 4., 5.]
        );
        let zero = homogeneous_spans(
            &ctx,
            0,
            &[0., 1., 2.],
            &[[0., 0., 0., 1.], [1., 0., 0., 1.]],
        )
        .expect("resource allocation did not fail")
        .unwrap();
        assert_eq!(zero.len(), 2);
    }
    #[test]
    fn numerical_followup_boundary_certificate_keeps_small_cross_products() {
        for w in [1.0, 1e-200, 1e200] {
            let a = [[0., 0., 0., w], [w, 0., 0., w]];
            let b = [[0., 2.0 * w, 0., w], [w, 2.0 * w, 0., w]];
            assert_eq!(
                boundaries_within_resolution(
                    &cadmpeg_test_support::service_decode_context(),
                    &a,
                    &b,
                    0.001
                ),
                Ok(Some(false))
            );
            assert_eq!(
                boundaries_within_resolution(
                    &cadmpeg_test_support::service_decode_context(),
                    &a,
                    &a,
                    0.0
                ),
                Ok(Some(true))
            );
        }
    }

    #[test]
    fn numerical_0922b_bezier_insertion_preserves_wide_knot_units() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let controls = vec![
            [0., 0., 0., 1.],
            [1., 0., 0., 1.],
            [1., 1., 0., 1.],
            [2., 1., 0., 1.],
        ];
        let expected = homogeneous_spans(&ctx, 2, &[-1., -1., -1., 0., 1., 1., 1.], &controls)
            .expect("resource allocation did not fail")
            .unwrap();
        let actual = homogeneous_spans(
            &ctx,
            2,
            &[-1e308, -1e308, -1e308, 0., 1e308, 1e308, 1e308],
            &controls,
        )
        .expect("resource allocation did not fail")
        .unwrap();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected.iter()) {
            assert_eq!(actual.controls, expected.controls);
            assert_eq!(actual.domain.map(|t| t / 1e308), expected.domain);
        }
    }

    #[test]
    fn bezier_extraction_keeps_a_single_clamped_or_unclamped_linear_span() {
        let ctx = cadmpeg_test_support::service_decode_context();
        let controls = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];
        for knots in [[0.0, 0.0, 1.0, 1.0], [-1.0, 0.0, 1.0, 2.0]] {
            let output = homogeneous_spans(&ctx, 1, &knots, &controls)
                .expect("span admission")
                .expect("one active span");
            assert_eq!(output.len(), 1);
            assert_eq!(output[0].domain, [0.0, 1.0]);
            assert_eq!(output[0].controls, controls);
        }
    }

    #[test]
    fn positive_controls_normalize_weights_into_homogeneous_rows() {
        use crate::math::Point3;
        let ctx = cadmpeg_test_support::service_decode_context();
        let points = [Point3::new(0.0, 1.0, 2.0), Point3::new(3.0, 4.0, 5.0)];
        let weights = [2.0, 1.0];
        let output =
            super::positive_controls(&ctx, &points, Some(&weights), "Bezier positive controls")
                .expect("control admission")
                .expect("positive weights");
        assert_eq!(&*output, [[0.0, 1.0, 2.0, 1.0], [1.5, 2.0, 2.5, 0.5]]);
    }
}
