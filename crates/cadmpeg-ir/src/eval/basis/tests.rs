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
        crate::eval::basis::bspline_span(&ctx, &knots, usize::MAX - 1, usize::MAX, 0.5)
            .expect("shape check"),
        None
    );
}

#[test]
fn low_degree_second_derivative_basis_keeps_zeros_inline() {
    crate::eval::test_support::with_policy(cadmpeg_core::decode::DecodePolicy::service(), |ctx| {
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
        assert!(matches!(
            constant,
            crate::eval::decode::SupportValues::Inline { .. }
        ));
        assert!(matches!(
            linear,
            crate::eval::decode::SupportValues::Inline { .. }
        ));
        assert_eq!(&*constant, &[0.0]);
        assert_eq!(&*linear, &[0.0, 0.0]);
    });
}

#[test]
fn repeated_inline_basis_and_derivatives_do_not_admit_collections() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    with_policy(policy, |ctx| {
        for degree in [0, 1, 2, 3, 4, 7, 15] {
            let mut knots = vec![0.0; degree + 1];
            knots.extend(vec![1.0; degree + 1]);
            for _ in 0..100 {
                let scratch = crate::eval::decode::Scratch::new(ctx);
                let basis = super::bspline_basis(&scratch, &knots, degree, degree, 0.5).unwrap();
                let first =
                    super::bspline_basis_derivative(&scratch, &knots, degree, degree, 0.5).unwrap();
                let second =
                    super::bspline_basis_second_derivative(&scratch, &knots, degree, degree, 0.5)
                        .unwrap();
                let scaled = super::bspline_basis_scaled_derivatives(
                    &scratch,
                    &knots,
                    degree,
                    degree,
                    0.5,
                    crate::scalar::PositiveReal::ONE,
                )
                .unwrap();
                assert!((basis.iter().sum::<f64>() - 1.0).abs() < f64::EPSILON * 64.0);
                assert!(first.iter().sum::<f64>().abs() < f64::EPSILON * 512.0);
                assert!(second.iter().sum::<f64>().abs() < f64::EPSILON * 4096.0);
                assert_eq!(&*scaled.first, &*first);
                assert_eq!(&*scaled.second, &*second);
                scratch.finish(()).unwrap();
            }
        }
        assert!(
            ctx.charge_work_limit(u64::MAX, "measure basis work")
                .unwrap_err()
                .used
                > 0
        );
    });
}

#[test]
fn basis_above_inline_support_keeps_collection_admission() {
    let degree = crate::eval::decode::INLINE_SUPPORT;
    let mut knots = vec![0.0; degree + 1];
    knots.extend(vec![1.0; degree + 1]);
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let result = with_policy(policy, |ctx| {
        let scratch = crate::eval::decode::Scratch::new(ctx);
        let result = super::bspline_basis(&scratch, &knots, degree, degree, 0.5);
        scratch.finish(result).map_err(CodecError::from)
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(resource))
        if resource.operation == "IR B-spline basis"));
}

#[test]
fn inline_quartic_basis_keeps_fill_and_recurrence_work() {
    let knots = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
    // Five initialization visits, one seed, 1 + 2 + 3 + 4 recurrence visits,
    // and four writes that advance the support window.
    for cap in [19, 20] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let result = with_policy(policy, |ctx| {
            let scratch = crate::eval::decode::Scratch::new(ctx);
            let basis = super::bspline_basis(&scratch, &knots, 4, 4, 0.5);
            scratch.finish(basis).map_err(CodecError::from)
        });
        if cap == 19 {
            assert!(matches!(result, Err(CodecError::ResourceLimit(limit))
                if limit.operation == "IR B-spline basis work"));
        } else {
            assert_eq!(
                &*result.unwrap().unwrap(),
                &[1.0 / 16.0, 0.25, 0.375, 0.25, 1.0 / 16.0]
            );
        }
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
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 2;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    with_policy(policy, |ctx| {
        assert_eq!(
            super::bspline_span(ctx, &knots, 1, 8, 1.5).expect("two comparisons"),
            Some(2)
        );
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
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 6;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    with_policy(policy, |ctx| {
        let mut values = [0.0; 3];
        assert_eq!(
            super::fill_bspline_basis(ctx, &knots, 2, 2, 0.5, &mut values)
                .expect("six recurrence operations"),
            Some(())
        );
        assert_eq!(values, [0.25, 0.5, 0.25]);
    });
}

#[test]
fn finite_basis_inspection_preserves_refusals_and_stops_at_first_nonfinite() {
    use crate::eval::decode::Scratch;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, ResourceDimension};
    for cap in 0..3 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let scratch = Scratch::new(&ctx);
        assert_eq!(super::all_finite(&scratch, &[0.25, 0.5, 0.25]), None);
        let limit = scratch.refused().expect("inspection refusal");
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!(limit.operation, "IR B-spline finite basis inspection");
        drop(scratch);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)
        );
    }
    for nonfinite in [false, true] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 3;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let scratch = Scratch::new(&ctx);
        let middle = if nonfinite { f64::NAN } else { 0.5 };
        assert_eq!(
            super::all_finite(&scratch, &[0.25, middle, 0.25]),
            Some(!nonfinite)
        );
        if nonfinite {
            ctx.charge_work_limit(1, "unused final basis visit")
                .expect("third value was not visited");
        }
        drop(scratch);
        ctx.finish_session().expect("inspection completed");
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    with_policy(policy, |ctx| {
        let scratch = Scratch::new(ctx);
        assert_eq!(super::all_finite(&scratch, &[]), Some(true));
        assert_eq!(super::all_finite(&scratch, &[1.0]), Some(true));
        assert_eq!(super::all_finite(&scratch, &[0.25, 0.75]), Some(true));
        assert_eq!(
            super::all_finite(&scratch, &[0.25, f64::INFINITY]),
            Some(false)
        );
        assert_eq!(scratch.refused(), None);
    });
}

#[test]
fn basis_leaf_boundaries_preserve_original_fused_refusal() {
    use crate::eval::decode::Scratch;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let original = ctx
        .charge_work_limit(1, "original basis refusal")
        .unwrap_err();
    for (knots, degree, count, parameter) in [
        (&[][..], usize::MAX, 0, 0.0),
        (&[0.0, 1.0][..], 0, 1, 0.0),
        (&[0.0, 1.0][..], 0, 1, 1.0),
        (&[0.0, 0.0, 1.0, 1.0][..], 1, 2, 0.5),
    ] {
        assert_eq!(
            super::bspline_span(&ctx, knots, degree, count, parameter),
            Err(original)
        );
    }
    for degree in [0, 1, usize::MAX] {
        let mut values = [0.0; 2];
        assert_eq!(
            super::fill_bspline_basis(&ctx, &[], degree, 0, 0.0, &mut values),
            Err(original)
        );
        assert_eq!(values, [0.0; 2]);
    }
    let scratch = Scratch::new(&ctx);
    for values in [&[][..], &[1.0][..], &[0.25, 0.75][..], &[f64::NAN][..]] {
        assert_eq!(super::all_finite(&scratch, values), None);
        assert_eq!(scratch.refused(), Some(original));
    }
    drop(scratch);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn cubic_and_quartic_bases_use_fixed_storage() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    with_policy(policy, |ctx| {
        let scratch = crate::eval::decode::Scratch::new(ctx);
        let cubic_knots = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
        let cubic = super::bspline_basis(&scratch, &cubic_knots, 3, 3, 0.5).unwrap();
        assert!(matches!(
            cubic,
            crate::eval::decode::SupportValues::Inline { .. }
        ));
        assert_eq!(&*cubic, &[0.125, 0.375, 0.375, 0.125]);
        assert_eq!(
            &*super::bspline_basis_derivative(&scratch, &cubic_knots, 3, 3, 0.5).unwrap(),
            &[-0.75, -0.75, 0.75, 0.75]
        );
        assert_eq!(
            &*super::bspline_basis_second_derivative(&scratch, &cubic_knots, 3, 3, 0.5).unwrap(),
            &[3.0, -3.0, -3.0, 3.0]
        );
        let higher_knots = [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0];
        let quartic = super::bspline_basis(&scratch, &higher_knots, 4, 4, 0.5).unwrap();
        assert_eq!(&*quartic, &[0.0625, 0.25, 0.375, 0.25, 0.0625]);
        assert!(scratch.refused().is_none());
    });
}
