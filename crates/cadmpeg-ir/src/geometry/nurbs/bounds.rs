// SPDX-License-Identifier: Apache-2.0
//! Bounds for positive-weight rational control polygons.

use crate::math::sum::ExactSignedSum;
use crate::scalar::FiniteReal;

/// Global rational curve speed bound about `origin` over the active knot domain.
/// Common weight scaling is removed before products are formed.
pub fn speed_bound<const N: usize>(
    degree: u32,
    knots: &[f64],
    points: &[[f64; N]],
    weights: &[f64],
    origin: [f64; N],
) -> Option<FiniteReal> {
    let order = usize::try_from(degree).ok()?;
    if points.len() <= order
        || weights.len() != points.len()
        || knots.len() != points.len().checked_add(order)?.checked_add(1)?
        || knots.iter().chain(origin.iter()).any(|v| !v.is_finite())
        || knots.windows(2).any(|p| p[0] > p[1])
        || weights.iter().any(|w| !w.is_finite() || *w <= 0.0)
        || points.iter().flatten().any(|v| !v.is_finite())
    {
        return None;
    }
    let scale = weights.iter().copied().fold(0.0_f64, f64::max);
    let minimum = weights
        .iter()
        .map(|w| w / scale)
        .fold(f64::INFINITY, f64::min);
    if minimum <= 0.0 {
        return None;
    }
    let mut maximum_radius = 0.0_f64;
    let mut numerator_speed = 0.0_f64;
    let mut weight_speed = 0.0_f64;
    let mut previous: Option<[f64; N]> = None;
    for (index, point) in points.iter().enumerate() {
        let weight = weights[index] / scale;
        let relative: [f64; N] = std::array::from_fn(|axis| point[axis] - origin[axis]);
        let weighted: [f64; N] = std::array::from_fn(|axis| weight * relative[axis]);
        if relative.iter().chain(&weighted).any(|v| !v.is_finite())
            || relative
                .iter()
                .zip(&weighted)
                .any(|(raw, product)| *raw != 0.0 && *product == 0.0)
        {
            return None;
        }
        maximum_radius = maximum_radius.max(weighted.iter().fold(0.0_f64, |r, v| r.hypot(*v)));
        if let Some(first) = previous {
            let lower = knots[index];
            let upper = knots[index + order];
            if upper > lower {
                let mut width = ExactSignedSum::default();
                width.add_product(upper, 1.0);
                width.add_product(lower, -1.0);
                let width = width.finish()?;
                let derivative = |delta: f64| {
                    if !delta.is_finite() {
                        return None;
                    }
                    let mut numerator = ExactSignedSum::default();
                    numerator.add_product(delta, f64::from(degree));
                    numerator.finish().map_or(Some(0.0), |value| {
                        value.quotient(width).map(FiniteReal::get)
                    })
                };
                let delta = weighted
                    .iter()
                    .zip(first)
                    .fold(0.0_f64, |r, (b, a)| r.hypot(b - a));
                numerator_speed = numerator_speed.max(derivative(delta)?);
                weight_speed =
                    weight_speed.max(derivative((weight - weights[index - 1] / scale).abs())?);
            }
        }
        previous = Some(weighted);
    }
    if !maximum_radius.is_finite() {
        return None;
    }
    let mut numerator = ExactSignedSum::default();
    numerator.add_product(maximum_radius, weight_speed);
    let mut denominator = ExactSignedSum::default();
    denominator.add_product(minimum, minimum);
    let correction = match numerator.finish() {
        Some(value) => value.quotient(denominator.finish()?)?.get(),
        None => 0.0,
    };
    FiniteReal::new(numerator_speed / minimum + correction)
}

#[cfg(test)]
mod tests {
    use super::speed_bound;
    #[test]
    fn numerical_followup_speed_bound_ignores_common_weight_scale() {
        for w in [1.0, 1e-200, 1e200, 1e308, f64::from_bits(1)] {
            assert_eq!(
                speed_bound(
                    1,
                    &[0., 0., 1., 1.],
                    &[[0., 0.], [1., 0.]],
                    &[w, w],
                    [0., 0.]
                )
                .map(crate::scalar::FiniteReal::get),
                Some(1.0)
            );
        }
    }

    #[test]
    fn numerical_audit_speed_bound_keeps_wide_finite_knots() {
        let speed = super::speed_bound(
            1,
            &[-1e308, -1e308, 1e308, 1e308],
            &[[0., 0.], [1., 0.]],
            &[1., 1.],
            [0., 0.],
        )
        .unwrap()
        .get();
        assert!((speed * 1e308 - 0.5).abs() <= 8. * f64::EPSILON);
    }
    #[test]
    fn numerical_audit_speed_bound_refuses_unrepresentable_control_differences() {
        assert_eq!(
            super::speed_bound(
                1,
                &[0., 0., 1., 1.],
                &[[-1e308, 0.], [1e308, 0.]],
                &[1., 1.],
                [0., 0.]
            ),
            None
        );
    }
}
