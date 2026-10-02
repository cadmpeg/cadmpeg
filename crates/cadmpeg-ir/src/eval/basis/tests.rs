// SPDX-License-Identifier: Apache-2.0

use crate::eval::test_support::with_policy;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;

#[test]
fn knot_span_refuses_oversized_degree_and_count_without_overflow() {
    let knots = [0.0, 1.0];
    let ctx = cadmpeg_test_support::service_decode_context();
    assert_eq!(
        crate::eval::basis::bspline_span(&ctx, &knots, usize::MAX, 1, 0.5).expect("shape check"),
        None
    );
    assert_eq!(
        crate::eval::basis::bspline_span(&ctx, &knots, usize::MAX - 1, usize::MAX, 0.5).expect("shape check"),
        None
    );
}

#[test]
fn low_degree_second_derivative_basis_borrows_zeros() {
    crate::eval::test_support::with_policy(cadmpeg_core::decode::DecodePolicy::service(), |ctx| {
        use std::borrow::Cow;

        let constant = crate::eval::basis::bspline_basis_second_derivative(
            &crate::eval::decode::Scratch::new(ctx),
            &[],
            0,
            0,
            0.0,
        )
        .expect("degree-zero second derivative");
        let linear = crate::eval::basis::bspline_basis_second_derivative(
            &crate::eval::decode::Scratch::new(ctx),
            &[],
            1,
            0,
            0.0,
        )
        .expect("degree-one second derivative");
        assert!(matches!(constant, Cow::Borrowed(_)));
        assert!(matches!(linear, Cow::Borrowed(_)));
        assert_eq!(constant.as_ref(), &[0.0]);
        assert_eq!(linear.as_ref(), &[0.0, 0.0]);
    });
}

#[test]
fn admitted_scaled_derivatives_refuse_each_collection() {
    crate::eval::test_support::with_policy(cadmpeg_core::decode::DecodePolicy::service(), |ctx| {
        for (degree, cap, operation) in [
            (0, 0, "IR scaled B-spline first basis"),
            (0, 1, "IR scaled B-spline second basis"),
            (1, 1, "IR scaled B-spline derivative basis"),
            (1, 3, "IR scaled B-spline second basis"),
            (2, 2, "IR scaled B-spline derivative basis"),
            (2, 4, "IR scaled B-spline derivative basis"),
            (2, 7, "IR scaled B-spline derivative basis"),
        ] {
            let mut knots = vec![0.0; degree + 1];
            knots.extend(vec![1.0; degree + 1]);
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let result = with_policy(policy, |ctx| {
                let scratch = crate::eval::decode::Scratch::new(ctx);
                let result = crate::eval::basis::bspline_basis_scaled_derivatives(
                    &scratch,
                    &knots,
                    degree,
                    degree,
                    0.5,
                    crate::scalar::PositiveReal::ONE,
                );
                scratch.finish(result).map_err(CodecError::from)
            });
            assert!(
                matches!(result, Err(CodecError::ResourceLimit(resource)) if resource.operation == operation)
            );
            let result = with_policy(DecodePolicy::service(), |ctx| {
                let scratch = crate::eval::decode::Scratch::new(ctx);
                let result = crate::eval::basis::bspline_basis_scaled_derivatives(
                    &scratch,
                    &knots,
                    degree,
                    degree,
                    0.5,
                    crate::scalar::PositiveReal::ONE,
                );
                scratch.finish(result).map_err(CodecError::from)
            })
            .expect("service");
            assert_eq!(
                result,
                crate::eval::basis::bspline_basis_scaled_derivatives(
                    &crate::eval::decode::Scratch::new(ctx),
                    &knots,
                    degree,
                    degree,
                    0.5,
                    crate::scalar::PositiveReal::ONE
                )
            );
            assert!(result.is_some());
        }
    });
}

#[test]
fn admitted_derivative_arithmetic_refuses_each_work_loop() {
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    for (kind, cap, operation) in [
        (0, 2, "IR B-spline derivative work"),
        (1, 1, "IR B-spline derivative work"),
        (1, 4, "IR B-spline second derivative work"),
        (2, 2, "IR scaled B-spline derivative work"),
    ] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let result = with_policy(policy, |ctx| {
            let scratch = crate::eval::decode::Scratch::new(ctx);
            let result = match kind {
                0 => crate::eval::basis::bspline_basis_derivative(&scratch, &knots, 2, 2, 0.5)
                    .map(|_| ()),
                1 => {
                    crate::eval::basis::bspline_basis_second_derivative(&scratch, &knots, 2, 2, 0.5)
                        .map(|_| ())
                }
                _ => crate::eval::basis::bspline_basis_scaled_derivatives(
                    &scratch,
                    &knots,
                    2,
                    2,
                    0.5,
                    crate::scalar::PositiveReal::ONE,
                )
                .map(|_| ()),
            };
            scratch.finish(result).map_err(CodecError::from)
        });
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(resource)) if resource.operation == operation)
        );
    }
}

#[test]
fn knot_span_search_preserves_each_comparison_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, ResourceDimension};
    let knots = [0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 7.0];
    for cap in 0..2 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let limit = super::bspline_span(&ctx, &knots, 1, 8, 1.5).expect_err("search comparison");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "IR B-spline span search");
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    with_policy(policy, |ctx| {
        assert_eq!(super::bspline_span(ctx, &knots, 1, 8, 1.5).expect("two comparisons"), Some(2));
    });
}

#[test]
fn basis_recurrence_preserves_each_write_and_blend_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, ResourceDimension};
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    for cap in 0..6 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut values = [0.0; 3];
        let limit = super::fill_bspline_basis(&ctx, &knots, 2, 2, 0.5, &mut values)
            .expect_err("each recurrence operation requires admission");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "IR B-spline basis work");
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 6;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    with_policy(policy, |ctx| {
        let mut values = [0.0; 3];
        assert_eq!(super::fill_bspline_basis(ctx, &knots, 2, 2, 0.5, &mut values)
            .expect("six recurrence operations"), Some(()));
        assert_eq!(values, [0.25, 0.5, 0.25]);
    });
}
