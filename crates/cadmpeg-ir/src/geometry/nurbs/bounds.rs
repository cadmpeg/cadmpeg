// SPDX-License-Identifier: Apache-2.0
//! Bounds for positive-weight rational control polygons.

use crate::math::{multiply_divide, sum::ExactSignedSum};

/// Global rational curve speed bound about `origin` over the active knot domain.
/// Common weight scaling is removed before products are formed.
pub fn speed_bound<const N: usize>(
    degree: u32,
    knots: &[f64],
    points: &[[f64; N]],
    weights: &[f64],
    origin: [f64; N],
) -> Option<f64> {
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
            let width = knots[index + order] - knots[index];
            if !width.is_finite() || width < 0.0 {
                return None;
            }
            if width > 0.0 {
                let delta = weighted
                    .iter()
                    .zip(first)
                    .fold(0.0_f64, |r, (b, a)| r.hypot(b - a));
                numerator_speed =
                    numerator_speed.max(multiply_divide(delta, f64::from(degree), width)?);
                weight_speed = weight_speed.max(multiply_divide(
                    (weight - weights[index - 1] / scale).abs(),
                    f64::from(degree),
                    width,
                )?);
            }
        }
        previous = Some(weighted);
    }
    let mut numerator = ExactSignedSum::default();
    numerator.add_product(maximum_radius, weight_speed);
    let mut denominator = ExactSignedSum::default();
    denominator.add_product(minimum, minimum);
    let correction = match numerator.finish() {
        Some(value) => value.quotient(denominator.finish()?)?,
        None => 0.0,
    };
    let bound = numerator_speed / minimum + correction;
    bound.is_finite().then_some(bound)
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
                ),
                Some(1.0)
            );
        }
    }
}
