// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn normalized_rational_surface_higher_reuses_actual_base_and_all_mixed_laws() {
    // H=(u,v,0), W=1+u+v. Third's homogeneous Taylor term is
    // (u,v,0)*(u+v)^2, Fourth's is -(u,v,0)*(u+v)^3.
    let surface = bilinear(
        vec![vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.5, 0.0)],
            vec![Point3::new(0.5, 0.0, 0.0), Point3::new(1.0 / 3.0, 1.0 / 3.0, 0.0)]],
        vec![vec![1.0, 2.0], vec![2.0, 3.0]],
    );
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        let local = nurbs_surface_local(&scratch, &surface, 0.0, 0.0).unwrap();
        for orders in [higher::Orders::Third, higher::Orders::Fourth, higher::Orders::ThirdAndFourth] {
            let actual = higher::evaluate(&scratch, &local, orders).unwrap();
            if !matches!(orders, higher::Orders::Fourth) {
                for (actual, (x, y)) in actual.third().unwrap().into_iter().zip([
                    (6.0, 0.0), (4.0, 2.0), (2.0, 4.0), (0.0, 6.0),
                ]) { close(actual.get(), Vector3::new(x, y, 0.0)); }
            } else { assert_eq!(actual.third(), Err(EvaluationFailure::NoValue)); }
            if !matches!(orders, higher::Orders::Third) {
                for (actual, (x, y)) in actual.fourth().unwrap().into_iter().zip([
                    (-24.0, 0.0), (-18.0, -6.0), (-12.0, -12.0), (-6.0, -18.0), (0.0, -24.0),
                ]) { close(actual.get(), Vector3::new(x, y, 0.0)); }
            } else { assert_eq!(actual.fourth(), Err(EvaluationFailure::NoValue)); }
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn normalized_rational_joint_higher_uses_real_38_visit_and_original_fuse() {
    let surface = polynomial_extended::small_span(true, 2.0_f64.powi(-1000));
    // Normalized u support5: two initialization walks10, four rows4,
    // fourteen cells, one rectangular10-pole walk. No repeated H00 sum.
    for cap in [37, 38] {
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let local = nurbs_surface_local(&scratch, &surface, 0.0, 0.5).unwrap();
        assert!(matches!(local.first(&scratch), Err(EvaluationFailure::NonFinite(()))));
        let budget = ctx.work_budget(cap);
        let actual = EvaluationAdmission::Decode(&ctx).within_work_slice(&budget, |admission|
            higher::evaluate(&Scratch::new(admission), &local, higher::Orders::ThirdAndFourth));
        assert_eq!(budget.consumed(), usize::try_from(cap).unwrap());
        let original = if cap == 37 {
            let Err(EvaluationFailure::ResourceLimit(limit)) = actual else { panic!("last actual Rational pole visit must refuse"); };
            assert_eq!(limit.dimension, ResourceDimension::Codec("geometry evaluation work slice"));
            assert_eq!(limit.operation, "geometry evaluation work slice");
            assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
            assert!(matches!(higher::evaluate(&scratch, &local, higher::Orders::ThirdAndFourth),
                Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == limit));
            Some(limit)
        } else {
            let actual = actual.unwrap();
            close(actual.third().unwrap()[3].get(), Vector3::new(0.0, 0.0, 12.0 / 1.5_f64.powi(4)));
            close(actual.fourth().unwrap()[0].get(), Vector3::new(0.0, 24.0 * 2.0_f64.powi(40), 0.0));
            close(actual.fourth().unwrap()[4].get(), Vector3::new(0.0, 0.0, -48.0 / 1.5_f64.powi(5)));
            None
        };
        drop(local); drop(scratch); drop(budget);
        match original {
            Some(limit) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)),
            None => { ctx.finish_session().unwrap(); }
        }
    }
    for cap in [134, 135] {
        let budget = WorkBudget::new(cap);
        let actual = EvaluationAdmission::Standard.within_work_slice(&budget, |admission|
            nurbs_surface_requested_jet(&Scratch::new(admission), &surface, 0.0, 0.5, SurfaceRequest::Fourth)).unwrap();
        assert_eq!(actual.jet.first, Err(EvaluationFailure::NonFinite(())));
        if cap == 134 {
            assert_eq!(actual.higher.third(), Err(EvaluationFailure::NoValue));
            assert_eq!(actual.higher.fourth(), Err(EvaluationFailure::NoValue));
        } else {
            assert!(actual.higher.third().is_ok());
            close(actual.higher.fourth().unwrap()[0].get(), Vector3::new(0.0, 24.0 * 2.0_f64.powi(40), 0.0));
        }
        assert_eq!(budget.consumed(), cap);
    }
}

#[test]
fn actual_unavailable_normalized_fourth_retains_third_and_old_supported_path() {
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for exponent in [-400, -200] {
        let h = 2.0_f64.powi(exponent);
        let u = NurbsSurfaceAxis::new(4,
            vec![0.0, 0.0, 0.0, 0.0, 0.0, h, 1.0, 2.0, 3.0, 4.0, 4.0, 4.0, 4.0, 4.0], false);
        let v = NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
        let poles = (0..9).map(|i| (0..2).map(|j: u32|
            Point3::new(if i == 4 { 1.0 } else { 0.0 }, 0.0, f64::from(j))
        ).collect()).collect();
        let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), u, v,
            NurbsSurfaceLanes::new(poles, Some(vec![vec![1.0, 2.0]; 9])), false).unwrap().unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let scratch = Scratch::new(admission);
            let local = nurbs_surface_local(&scratch, &surface, 0.0, 0.5).unwrap();
            // B4=t^4/(6*h) on this actual span. D3 at zero is0 and
            // physical D4=4/h is finite. At h=2^-400 normalized D4=4*h^3
            // is lost, and the separate physical B0'''=-24/h^3 overflows.
            let third = higher::evaluate(&scratch, &local, higher::Orders::Third).unwrap();
            let both = higher::evaluate(&scratch, &local, higher::Orders::ThirdAndFourth).unwrap();
            assert_eq!(third.third(), both.third());
            for lane in &both.third().unwrap()[..3] { assert_eq!(*lane, FiniteVector3::ZERO); }
            close(both.third().unwrap()[3].get(), Vector3::new(0.0, 0.0, 12.0 / 1.5_f64.powi(4)));
            // Differing zero-basis poles prevent a false constant-coordinate proof.
            let original = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, SurfaceRequest::Fourth).unwrap();
            if exponent == -400 {
                assert_eq!(both.fourth(), Err(EvaluationFailure::NoValue));
                assert_eq!(original.higher.third(), Err(EvaluationFailure::NonFinite(())));
                assert_eq!(original.higher.fourth(), Err(EvaluationFailure::NoValue));
            } else {
                assert_eq!(both.fourth().unwrap()[0].get(), Vector3::new(4.0 / h, 0.0, 0.0));
                assert_eq!(original.higher.fourth().unwrap()[0].get(), Vector3::new(4.0 / h, 0.0, 0.0));
            }
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn actual_rational_higher_keeps_lower_orders_and_independent_fourth_overflow() {
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for amplitude in [2.0_f64.powi(-1000), f64::MAX] {
        let surface = polynomial_extended::small_span(true, amplitude);
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let scratch = Scratch::new(admission);
            let actual = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, SurfaceRequest::Fourth).unwrap();
            assert_eq!(actual.jet.point.get(), Point3::new(0.0, 0.0, 2.0 / 3.0));
            assert_eq!(actual.jet.first, Err(EvaluationFailure::NonFinite(())));
            for lane in &actual.higher.third().unwrap()[..3] { assert_eq!(*lane, FiniteVector3::ZERO); }
            close(actual.higher.third().unwrap()[3].get(), Vector3::new(0.0, 0.0, 12.0 / 1.5_f64.powi(4)));
            if amplitude == f64::MAX {
                // The true uuuu.y=24*MAX/h^4 overflows; Third at zero stays finite.
                assert_eq!(actual.higher.fourth(), Err(EvaluationFailure::NonFinite(())));
            } else {
                close(actual.higher.fourth().unwrap()[0].get(), Vector3::new(0.0, 24.0 * 2.0_f64.powi(40), 0.0));
            }
            for request in [SurfaceRequest::First, SurfaceRequest::Second, SurfaceRequest::Third] {
                let lower = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, request).unwrap();
                assert_eq!(lower.jet.point, actual.jet.point);
                assert_eq!(lower.jet.first, actual.jet.first);
                if request.needs_second() { assert_eq!(lower.jet.second, actual.jet.second); }
                if request.needs_third() { assert_eq!(lower.higher.third(), actual.higher.third()); }
                assert_eq!(lower.higher.fourth(), Err(EvaluationFailure::NoValue));
            }
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn actual_all_selected_rational_poles_prove_constant_extreme_coordinate() {
    // H.x=MAX*W over the whole selected support, with genuinely varying weights.
    // This source equality proves all higher x partials zero, even when the
    // separate finite homogeneous sums would round at different orders.
    let surface = bilinear(
        vec![vec![Point3::new(f64::MAX, 0.0, 0.0), Point3::new(f64::MAX, 0.0, 0.0)],
            vec![Point3::new(f64::MAX, 0.0, 0.0), Point3::new(f64::MAX, 0.0, 0.0)]],
        vec![vec![1.0, 2.0], vec![3.0, 4.0]],
    );
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        let local = nurbs_surface_local(&scratch, &surface, 0.25, 0.5).unwrap();
        assert_eq!(local.point, [FiniteReal::new(f64::MAX).unwrap(), FiniteReal::ZERO, FiniteReal::ZERO]);
        let actual = higher::evaluate(&scratch, &local, higher::Orders::ThirdAndFourth).unwrap();
        assert_eq!(actual.third(), Ok([FiniteVector3::ZERO; 4]));
        assert_eq!(actual.fourth(), Ok([FiniteVector3::ZERO; 5]));
    }
    ctx.finish_session().unwrap();
}
