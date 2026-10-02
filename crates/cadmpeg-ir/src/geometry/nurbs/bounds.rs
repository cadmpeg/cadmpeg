// SPDX-License-Identifier: Apache-2.0
//! Bounds for positive-weight rational control polygons.

use crate::math::sum::ExactSignedSum;
use crate::scalar::FiniteReal;
use cadmpeg_core::decode::{DecodeContext, ResourceLimit};

/// Global rational curve speed bound about `origin` over the active knot domain.
/// Common weight scaling is removed before products are formed. Absent
/// weights are read as 1.0 without allocating a weight array.
pub fn speed_bound<const N: usize>(
    ctx: &DecodeContext<'_>,
    degree: u32,
    knots: &[f64],
    points: &[[f64; N]],
    weights: Option<&[f64]>,
    origin: [f64; N],
) -> Result<Option<FiniteReal>, ResourceLimit> {
    if weights.is_some_and(|weights| weights.len() != points.len()) {
        return Ok(None);
    }
    speed_bound_by(
        ctx,
        degree,
        knots,
        points.len(),
        |index| points.get(index).copied(),
        |index| weights.map_or(1.0, |weights| weights[index]),
        origin,
    )
}

/// Evaluate a speed bound from admitted pole accessors without copying poles or weights.
pub fn speed_bound_by<const N: usize>(
    ctx: &DecodeContext<'_>,
    degree: u32,
    knots: &[f64],
    point_count: usize,
    point_at: impl Fn(usize) -> Option<[f64; N]>,
    weight_at: impl Fn(usize) -> f64,
    origin: [f64; N],
) -> Result<Option<FiniteReal>, ResourceLimit> {
    macro_rules! work {
        ($operation:expr) => {
            if let Err(limit) = ctx.charge_work_limit(1, $operation) {
                return Some(Err(limit));
            }
        };
    }
    let result = (|| {
    let order = usize::try_from(degree).ok()?;
    if point_count <= order || knots.len() != point_count.checked_add(order)?.checked_add(1)? {
        return None;
    }
    for value in knots.iter().chain(origin.iter()) {
        work!("IR rational speed finite coordinate visit");
        if !value.is_finite() { return None; }
    }
    for pair in knots.windows(2) {
        work!("IR rational speed knot order comparison");
        if pair[0] > pair[1] { return None; }
    }
    for index in 0..point_count {
        work!("IR rational speed pole validation");
        let weight = weight_at(index);
        if !weight.is_finite() || weight <= 0.0
            || point_at(index).is_none_or(|point| point.iter().any(|value| !value.is_finite())) {
            return None;
        }
    }
    let mut scale = 0.0_f64;
    for index in 0..point_count {
        work!("IR rational speed weight scale visit");
        scale = scale.max(weight_at(index));
    }
    let mut minimum = f64::INFINITY;
    for index in 0..point_count {
        work!("IR rational speed minimum weight visit");
        minimum = minimum.min(weight_at(index) / scale);
    }
    if minimum <= 0.0 {
        return None;
    }
    let mut maximum_radius = 0.0_f64;
    let mut numerator_speed = 0.0_f64;
    let mut weight_speed = 0.0_f64;
    let mut previous: Option<[f64; N]> = None;
    for index in 0..point_count {
        work!("IR rational speed control bound visit");
        let point = point_at(index)?;
        let weight = weight_at(index) / scale;
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
                        value.quotient(width).ok().map(FiniteReal::get)
                    })
                };
                let delta = weighted
                    .iter()
                    .zip(first)
                    .fold(0.0_f64, |r, (b, a)| r.hypot(b - a));
                numerator_speed = numerator_speed.max(derivative(delta)?);
                weight_speed =
                    weight_speed.max(derivative((weight - weight_at(index - 1) / scale).abs())?);
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
        Some(value) => value.quotient(denominator.finish()?).ok()?.get(),
        None => 0.0,
    };
    FiniteReal::new(numerator_speed / minimum + correction).map(Ok)
    })();
    result.transpose()
}

#[cfg(test)]
mod tests {
    use super::speed_bound;

    #[test]
    fn rational_speed_bound_admits_every_pass_without_allocating() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        use std::cell::Cell;
        let knots = [0., 0., 1., 1.];
        let points = [[0., 0.], [1., 0.]];
        // Six finite coordinates, three knot comparisons, and four two-pole passes.
        let work = 6 + 3 + 4 * 2;
        for cap in 0..work {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let called = Cell::new(false);
            let limit = super::speed_bound_by(&ctx, 1, &knots, 2,
                |index| { called.set(true); points.get(index).copied() }, |_| 1., [0., 0.])
                .expect_err("every pass requires admission");
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            if cap <= 9 { assert!(!called.get()); }
            assert_eq!(limit.operation, if cap < 6 { "IR rational speed finite coordinate visit" }
                else if cap < 9 { "IR rational speed knot order comparison" }
                else if cap < 11 { "IR rational speed pole validation" }
                else if cap < 13 { "IR rational speed weight scale visit" }
                else if cap < 15 { "IR rational speed minimum weight visit" }
                else { "IR rational speed control bound visit" });
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
        }
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert_eq!(super::speed_bound(&ctx, 1, &knots, &points, None, [0., 0.])
            .expect("exact work").map(crate::scalar::FiniteReal::get), Some(1.));
        ctx.finish_session().expect("zero allocation and exact visits");
    }

    #[test]
    fn implicit_unit_weights_match_explicit_unit_weights() {
        let knots = [0.0, 0.0, 1.0, 1.0];
        let points = [[0.0, 0.0], [1.0, 0.0]];
        let implicit = speed_bound(&cadmpeg_test_support::service_decode_context(), 1, &knots, &points, None, [0.25, 0.0]).expect("speed bound admission");
        let explicit = speed_bound(&cadmpeg_test_support::service_decode_context(), 1, &knots, &points, Some(&[1.0, 1.0]), [0.25, 0.0]).expect("speed bound admission");
        assert_eq!(implicit, explicit);
    }

    #[test]
    fn numerical_followup_speed_bound_ignores_common_weight_scale() {
        for w in [1.0, 1e-200, 1e200, 1e308, f64::from_bits(1)] {
            assert_eq!(
                speed_bound(&cadmpeg_test_support::service_decode_context(),
                    1,
                    &[0., 0., 1., 1.],
                    &[[0., 0.], [1., 0.]],
                    Some(&[w, w]),
                    [0., 0.]
                ).expect("speed bound admission")
                .map(crate::scalar::FiniteReal::get),
                Some(1.0)
            );
        }
    }

    #[test]
    fn numerical_audit_speed_bound_keeps_wide_finite_knots() {
        let speed = super::speed_bound(&cadmpeg_test_support::service_decode_context(),
            1,
            &[-1e308, -1e308, 1e308, 1e308],
            &[[0., 0.], [1., 0.]],
            Some(&[1., 1.]),
            [0., 0.],
        ).expect("speed bound admission")
        .unwrap()
        .get();
        assert!((speed * 1e308 - 0.5).abs() <= 8. * f64::EPSILON);
    }
    #[test]
    fn numerical_audit_speed_bound_refuses_unrepresentable_control_differences() {
        assert_eq!(
            super::speed_bound(&cadmpeg_test_support::service_decode_context(),
                1,
                &[0., 0., 1., 1.],
                &[[-1e308, 0.], [1e308, 0.]],
                Some(&[1., 1.]),
                [0., 0.]
            ).expect("speed bound admission"),
            None
        );
    }
}
