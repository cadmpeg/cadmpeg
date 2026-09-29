// SPDX-License-Identifier: Apache-2.0

use crate::eval::test_support::with_policy;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;

#[test]
fn knot_span_refuses_oversized_degree_and_count_without_overflow() {
    let knots = [0.0, 1.0];
    assert_eq!(
        crate::eval::basis::bspline_span(&knots, usize::MAX, 1, 0.5),
        None
    );
    assert_eq!(
        crate::eval::basis::bspline_span(&knots, usize::MAX - 1, usize::MAX, 0.5),
        None
    );
}

#[test]
fn low_degree_second_derivative_basis_borrows_zeros() {
    use std::borrow::Cow;

    let constant = crate::eval::basis::bspline_basis_second_derivative(
        &crate::eval::admitted::Scratch::default(),
        &[],
        0,
        0,
        0.0,
    )
    .expect("degree-zero second derivative");
    let linear = crate::eval::basis::bspline_basis_second_derivative(
        &crate::eval::admitted::Scratch::default(),
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
}

#[test]
fn admitted_scaled_derivatives_refuse_each_collection() {
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
            let scratch = crate::eval::admitted::Scratch::new(ctx);
            let result = crate::eval::basis::bspline_basis_scaled_derivatives(
                &scratch,
                &knots,
                degree,
                degree,
                0.5,
                crate::scalar::PositiveReal::ONE,
            );
            scratch.finish(result)
        });
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(resource)) if resource.operation == operation)
        );
        let result = with_policy(DecodePolicy::service(), |ctx| {
            let scratch = crate::eval::admitted::Scratch::new(ctx);
            let result = crate::eval::basis::bspline_basis_scaled_derivatives(
                &scratch,
                &knots,
                degree,
                degree,
                0.5,
                crate::scalar::PositiveReal::ONE,
            );
            scratch.finish(result)
        })
        .expect("service");
        assert_eq!(
            result,
            crate::eval::basis::bspline_basis_scaled_derivatives(
                &crate::eval::admitted::Scratch::default(),
                &knots,
                degree,
                degree,
                0.5,
                crate::scalar::PositiveReal::ONE
            )
        );
        assert!(result.is_some());
    }
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
            let scratch = crate::eval::admitted::Scratch::new(ctx);
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
            scratch.finish(result)
        });
        assert!(
            matches!(result, Err(CodecError::ResourceLimit(resource)) if resource.operation == operation)
        );
    }
}
