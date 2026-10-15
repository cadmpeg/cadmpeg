// SPDX-License-Identifier: Apache-2.0

use crate::eval::test_support::with_policy;
use cadmpeg_core::decode::DecodePolicy;

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
fn cubic_and_quartic_bases_match_bernstein_values() {
    let policy = DecodePolicy::service();
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

#[test]
fn bases_partition_unity_and_unit_scaled_derivatives_match() {
    with_policy(DecodePolicy::service(), |ctx| {
        for degree in [0, 1, 2, 3, 4, 7, 15] {
            let mut knots = vec![0.0; degree + 1];
            knots.extend(vec![1.0; degree + 1]);
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
            scratch.unless_refused().unwrap();
        }
    });
}

#[test]
fn span_search_recurrence_and_finite_inspection_values() {
    with_policy(DecodePolicy::service(), |ctx| {
        let knots = [0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 7.0];
        assert_eq!(
            super::bspline_span(ctx, &knots, 1, 8, 1.5).unwrap(),
            Some(2)
        );
        let mut values = [0.0; 3];
        assert_eq!(
            super::fill_bspline_basis(ctx, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0], 2, 2, 0.5, &mut values)
                .unwrap(),
            Some(())
        );
        assert_eq!(values, [0.25, 0.5, 0.25]);
        let scratch = crate::eval::decode::Scratch::new(ctx);
        assert_eq!(super::all_finite(&scratch, &[]), Some(true));
        assert_eq!(super::all_finite(&scratch, &[0.25, 0.5, 0.25]), Some(true));
        assert_eq!(
            super::all_finite(&scratch, &[0.25, f64::NAN, 0.25]),
            Some(false)
        );
        assert_eq!(
            super::all_finite(&scratch, &[0.25, f64::INFINITY]),
            Some(false)
        );
    });
}
