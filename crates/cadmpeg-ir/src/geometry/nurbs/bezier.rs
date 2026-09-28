// SPDX-License-Identifier: Apache-2.0
//! Homogeneous Bezier extraction and rational boundary bounds.

use super::scratch;
use super::PoleValue;
use crate::features::FinitePoint3;
use crate::math::sum::ExactSignedSum;
use cadmpeg_core::decode::ResourceLimit;

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
/// coordinate product that would disappear.
/// The output polygon and its scratch have one control per admitted input pole; absent weights
/// are read as 1.0 without allocating a weight array.
pub fn positive_controls<P: PoleValue<FinitePoint3>>(
    points: &[P],
    weights: Option<&[f64]>,
) -> Result<Option<Vec<[f64; 4]>>, ResourceLimit> {
    if weights.is_some_and(|weights| points.len() != weights.len())
        || points.is_empty()
        || weights.is_some_and(|weights| {
            weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
        })
    {
        return Ok(None);
    }
    let weight_at = |index: usize| weights.map_or(1.0, |weights| weights[index]);
    let scale = weights.map_or(1.0, |weights| {
        weights.iter().copied().fold(0.0_f64, f64::max)
    });
    let mut output = Vec::new();
    scratch::reserve_exact(&mut output, points.len(), "Bezier positive controls")?;
    Ok(points
        .iter()
        .enumerate()
        .try_fold(output, |mut output, (index, point)| {
            let weight = weight_at(index) / scale;
            let point = point.admit()?.get();
            if weight == 0.0 {
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
            output.push(result);
            Some(output)
        }))
}

/// Knot and control scratch is bounded by the admitted knot and control counts.
fn insert_knot<const DIMENSION: usize, E: From<ResourceLimit>>(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; DIMENSION]>,
    value: f64,
    charge: &mut impl FnMut(usize, &'static str) -> Result<(), E>,
) -> Result<Option<()>, E> {
    let Some(last) = controls.len().checked_sub(1) else {
        return Ok(None);
    };
    let Some(span) = knots.iter().rposition(|knot| *knot <= value) else {
        return Ok(None);
    };
    let multiplicity = knots.iter().filter(|knot| **knot == value).count();
    let Some(first) = span.checked_sub(degree) else {
        return Ok(None);
    };
    let Some(tail) = span.checked_sub(multiplicity) else {
        return Ok(None);
    };
    if multiplicity > degree || first > last || tail > last {
        return Ok(None);
    }
    let Some(_) = controls.len().checked_add(1) else {
        return Ok(None);
    };
    charge(1, "Bezier knot insertion")?;
    scratch::reserve_exact(controls, 1, "Bezier knot insertion").map_err(E::from)?;
    charge(1, "Bezier inserted knot")?;
    scratch::reserve_exact(knots, 1, "Bezier inserted knot").map_err(E::from)?;
    controls.push([0.0; DIMENSION]);
    controls.copy_within(tail..=last, tail + 1);
    for index in (first + 1..=tail).rev() {
        let Some(alpha) =
            crate::math::parameter_fraction(value, knots[index], knots[index + degree])
                .map(crate::scalar::FiniteReal::get)
        else {
            return Ok(None);
        };
        controls[index] = std::array::from_fn(|axis| {
            (1.0 - alpha) * controls[index - 1][axis] + alpha * controls[index][axis]
        });
    }
    knots.insert(span + 1, value);
    Ok(Some(()))
}

/// Extract the active spans, including unclamped endpoints and discontinuities.
///
/// Knot multiplicity controls the control-point index of each span. A full
/// internal multiplicity starts a new polygon without sharing an endpoint.
/// Working knot and control copies are bounded by the admitted knot and control counts.
pub fn homogeneous_spans<const DIMENSION: usize>(
    degree: usize,
    knots: &[f64],
    controls: Vec<[f64; DIMENSION]>,
) -> Result<Option<Vec<HomogeneousBezierSpan<DIMENSION>>>, ResourceLimit> {
    homogeneous_spans_with_charge(degree, knots, controls, |_, _| Ok(()))
}

/// Extract active spans while charging each working vector before its allocation.
pub fn homogeneous_spans_with_charge<const DIMENSION: usize, E: From<ResourceLimit>>(
    degree: usize,
    knots: &[f64],
    mut controls: Vec<[f64; DIMENSION]>,
    mut charge: impl FnMut(usize, &'static str) -> Result<(), E>,
) -> Result<Option<Vec<HomogeneousBezierSpan<DIMENSION>>>, E> {
    let count = controls.len();
    let Some(expected_knots) = count
        .checked_add(degree)
        .and_then(|value| value.checked_add(1))
    else {
        return Ok(None);
    };
    if degree >= count
        || knots.len() != expected_knots
        || knots.iter().any(|value| !value.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
        || controls.iter().flatten().any(|value| !value.is_finite())
    {
        return Ok(None);
    }
    let domain = [knots[degree], knots[count]];
    if domain[0] >= domain[1] {
        return Ok(None);
    }
    charge(knots.len(), "Bezier knot copy")?;
    let mut knots_copy = scratch::filled(knots.len(), 0.0, "Bezier knot copy").map_err(E::from)?;
    knots_copy.copy_from_slice(knots);
    let mut knots = knots_copy;
    for endpoint in domain {
        while knots.iter().filter(|knot| **knot == endpoint).count() < degree + 1 {
            if insert_knot(degree, &mut knots, &mut controls, endpoint, &mut charge)?.is_none() {
                return Ok(None);
            }
        }
    }
    let mut internal = Vec::new();
    charge(knots.len(), "Bezier internal knots")?;
    scratch::reserve_exact(&mut internal, knots.len(), "Bezier internal knots").map_err(E::from)?;
    internal.extend(
        knots
            .iter()
            .copied()
            .filter(|knot| domain[0] < *knot && *knot < domain[1]),
    );
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            if insert_knot(degree, &mut knots, &mut controls, knot, &mut charge)?.is_none() {
                return Ok(None);
            }
        }
    }
    let mut spans = Vec::new();
    charge(controls.len() - degree, "Bezier spans")?;
    scratch::reserve_exact(&mut spans, controls.len() - degree, "Bezier spans").map_err(E::from)?;
    for span in degree..controls.len() {
        let interval = [knots[span], knots[span + 1]];
        if interval[0] < interval[1] && interval[0] >= domain[0] && interval[1] <= domain[1] {
            charge(degree + 1, "Bezier span controls")?;
            let mut span_controls =
                scratch::filled(degree + 1, [0.0; DIMENSION], "Bezier span controls")
                    .map_err(E::from)?;
            span_controls.copy_from_slice(&controls[span - degree..=span]);
            spans.push(HomogeneousBezierSpan {
                domain: interval,
                controls: span_controls,
            });
        }
    }
    Ok((!spans.is_empty()).then_some(spans))
}

/// Prove equal-degree positive-weight rational Bezier boundaries agree.
///
/// Bernstein cross-products are compared before converting their scale back
/// to `f64`, so a nonzero difference cannot disappear through underflow.
pub fn boundaries_within_resolution(
    first: &[[f64; 4]],
    second: &[[f64; 4]],
    resolution: f64,
) -> Result<Option<bool>, ResourceLimit> {
    Ok((|| {
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
    })())
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
        .expect("resource allocation did not fail")
        .unwrap();
        let controls = &spans[0].controls;
        let midpoint = (controls[0][1] + 2.0 * controls[1][1] + controls[2][1]) / 4.0;
        assert_eq!(midpoint, 0.75);
        let spans = homogeneous_spans(
            2,
            &[0., 0., 0., 1., 1., 1., 2., 2., 2.],
            (0..6).map(|i| [f64::from(i), 0., 0., 1.]).collect(),
        )
        .expect("resource allocation did not fail")
        .unwrap();
        assert_eq!(
            spans[1].controls.iter().map(|p| p[0]).collect::<Vec<_>>(),
            [3., 4., 5.]
        );
        let zero = homogeneous_spans(0, &[0., 1., 2.], vec![[0., 0., 0., 1.], [1., 0., 0., 1.]])
            .expect("resource allocation did not fail")
            .unwrap();
        assert_eq!(zero.len(), 2);
    }
    #[test]
    fn numerical_followup_boundary_certificate_keeps_small_cross_products() {
        for w in [1.0, 1e-200, 1e200] {
            let a = [[0., 0., 0., w], [w, 0., 0., w]];
            let b = [[0., 2.0 * w, 0., w], [w, 2.0 * w, 0., w]];
            assert_eq!(boundaries_within_resolution(&a, &b, 0.001), Ok(Some(false)));
            assert_eq!(boundaries_within_resolution(&a, &a, 0.0), Ok(Some(true)));
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
        let expected = homogeneous_spans(2, &[-1., -1., -1., 0., 1., 1., 1.], controls.clone())
            .expect("resource allocation did not fail")
            .unwrap();
        let actual = homogeneous_spans(
            2,
            &[-1e308, -1e308, -1e308, 0., 1e308, 1e308, 1e308],
            controls,
        )
        .expect("resource allocation did not fail")
        .unwrap();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert_eq!(actual.controls, expected.controls);
            assert_eq!(actual.domain.map(|t| t / 1e308), expected.domain);
        }
    }
}
