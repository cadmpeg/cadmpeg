// SPDX-License-Identifier: Apache-2.0
use super::*;

fn small_span(rational: bool, fourth_amplitude: f64) -> NurbsSurface {
    let width = 2.0_f64.powi(-260);
    let amplitude = 2.0_f64.powi(1000);
    let u = NurbsSurfaceAxis::new(4, vec![0.0, 0.0, 0.0, 0.0, 0.0, width, width, width, width, width], false);
    let v = NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    let poles = (0..5).map(|i: u32| (0..2).map(|j: u32|
        Point3::new(amplitude * f64::from(i) / 4.0,
            if i == 4 { fourth_amplitude } else { 0.0 }, f64::from(j))
    ).collect()).collect();
    let weights = rational.then(|| vec![vec![1.0, 2.0]; 5]);
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), u, v,
        NurbsSurfaceLanes::new(poles, weights), false).unwrap().unwrap()
}

fn expected_fourth() -> [[FiniteReal; 3]; 5] {
    let mut result = [[FiniteReal::ZERO; 3]; 5];
    // d^4 (2^-1000*(u/2^-260)^4) / du^4 = 24*2^40.
    result[0][1] = FiniteReal::new(24.0 * 2.0_f64.powi(40)).unwrap();
    result
}

#[test]
fn finite_polynomial_fourth_survives_actual_first_overflow_without_lower_replay() {
    let surface = small_span(false, 2.0_f64.powi(-1000));
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        let fourth = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, SurfaceRequest::Fourth).unwrap();
        assert_eq!(fourth.jet.point.get(), Point3::new(0.0, 0.0, 0.5));
        assert_eq!(fourth.jet.first, Err(EvaluationFailure::NonFinite(())));
        assert_eq!(fourth.higher.third().unwrap(), [FiniteVector3::ZERO; 4]);
        assert_eq!(fourth.higher.fourth().unwrap().map(|vector| vector.components()), expected_fourth());
        for request in [SurfaceRequest::First, SurfaceRequest::Second, SurfaceRequest::Third] {
            let lower = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, request).unwrap();
            assert_eq!(lower.jet.point, fourth.jet.point); assert_eq!(lower.jet.first, fourth.jet.first);
            if request.needs_second() { assert_eq!(lower.jet.second, fourth.jet.second); }
            if request.needs_third() { assert_eq!(lower.higher.third(), fourth.higher.third()); }
            assert_eq!(lower.higher.fourth(), Err(EvaluationFailure::NoValue));
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn missing_rational_raw_state_does_not_claim_nonfinite_fourth() {
    let surface = small_span(true, 2.0_f64.powi(-1000));
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let fourth = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, SurfaceRequest::Fourth).unwrap();
    assert_eq!(fourth.jet.first, Err(EvaluationFailure::NonFinite(())));
    assert_eq!(fourth.higher.third(), Err(EvaluationFailure::NoValue));
    assert_eq!(fourth.higher.fourth(), Err(EvaluationFailure::NoValue));
    let lower = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, SurfaceRequest::Second).unwrap();
    assert_eq!(fourth.jet.point, lower.jet.point); assert_eq!(fourth.jet.first, lower.jet.first);
    assert_eq!(fourth.jet.second, lower.jet.second);
}

#[test]
fn polynomial_fourth_normalized_stage_uses_real_38_visit_boundary() {
    let surface = small_span(false, 2.0_f64.powi(-1000));
    // Two support5 initializations10, four rows4, cells2+3+4+5=14,
    // and one ten-pole traversal:38. The other axis is fixed support2.
    for (orders, cap) in [polynomial_higher::Orders::Fourth, polynomial_higher::Orders::ThirdAndFourth]
        .into_iter().flat_map(|orders| [37, 38].map(|cap| (orders, cap))) {
        let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
        let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let local = nurbs_surface_local(&scratch, &surface, 0.0, 0.5).unwrap();
        assert!(matches!(local.first(&scratch), Err(EvaluationFailure::NonFinite(()))));
        let budget = ctx.work_budget(cap);
        let actual = EvaluationAdmission::Decode(&ctx).within_work_slice(&budget, |admission|
            polynomial_higher::evaluate(&Scratch::new(admission), &local, orders));
        assert_eq!(budget.consumed(), usize::try_from(cap).unwrap());
        let original = if cap == 37 {
            let Err(EvaluationFailure::ResourceLimit(limit)) = actual else { panic!("last real support visit must refuse"); };
            assert_eq!(limit.operation, "geometry evaluation work slice");
            assert_eq!(limit.dimension, ResourceDimension::Codec("geometry evaluation work slice"));
            assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
            assert!(matches!(polynomial_higher::evaluate(&scratch, &local, orders), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == limit));
            Some(limit)
        } else {
            let actual = actual.unwrap();
            assert_eq!(actual.fourth().unwrap().map(|vector| vector.components()), expected_fourth());
            if matches!(orders, polynomial_higher::Orders::ThirdAndFourth) {
                assert_eq!(actual.third().unwrap(), [FiniteVector3::ZERO; 4]);
            } else { assert_eq!(actual.third(), Err(EvaluationFailure::NoValue)); }
            None
        };
        drop(local); drop(scratch); drop(budget);
        match original {
            Some(limit) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)),
            None => { ctx.finish_session().unwrap(); }
        }
    }
    // Initial old stored cost10+3*(25+4)=97, then38 actual new steps.
    for cap in [134, 135] {
        let budget = WorkBudget::new(cap);
        let actual = EvaluationAdmission::Standard.within_work_slice(&budget, |admission|
            nurbs_surface_requested_jet(&Scratch::new(admission), &surface, 0.0, 0.5, SurfaceRequest::Fourth)).unwrap();
        assert_eq!(actual.jet.first, Err(EvaluationFailure::NonFinite(())));
        if cap == 134 { assert_eq!(actual.higher.fourth(), Err(EvaluationFailure::NoValue)); }
        else {
            assert_eq!(actual.higher.fourth().unwrap().map(|vector| vector.components()), expected_fourth());
            assert_eq!(actual.higher.third().unwrap(), [FiniteVector3::ZERO; 4]);
        }
        assert_eq!(budget.consumed(), cap);
    }
}

#[test]
fn normalized_polynomial_fourth_follows_all_tensor_coefficients_and_final_overflow() {
    let surface = quartic();
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        for (u, v) in [(0.0, 0.0), (0.3, 0.4)] {
            let scratch = Scratch::new(admission);
            let local = nurbs_surface_local(&scratch, &surface, u, v).unwrap();
            for orders in [polynomial_higher::Orders::Fourth, polynomial_higher::Orders::ThirdAndFourth] {
                let actual = polynomial_higher::evaluate(&scratch, &local, orders).unwrap();
                let fourth = actual.fourth().unwrap().map(|vector| vector.components());
                for (actual, expected) in fourth.into_iter().zip([24.0, 12.0, 12.0, 24.0, 120.0]) {
                    close(Vector3::new(actual[0].get(), actual[1].get(), actual[2].get()),
                        Vector3::new(0.0, 0.0, expected));
                }
                if matches!(orders, polynomial_higher::Orders::ThirdAndFourth) {
                    // S.z=u^4+2*u^3*v+3*u^2*v^2+4*u*v^3+5*v^4.
                    for (actual, expected) in actual.third().unwrap().into_iter().zip([
                        24.0 * u + 12.0 * v, 12.0 * u + 12.0 * v,
                        12.0 * u + 24.0 * v, 24.0 * u + 120.0 * v,
                    ]) { close(actual.get(), Vector3::new(0.0, 0.0, expected)); }
                }
            }
        }
        // The same exact quartic law now has amplitude MAX: its true fourth
        // 24*MAX/h^4 overflows. This is the new order's own range failure.
        let overflow = small_span(false, f64::MAX);
        let scratch = Scratch::new(admission);
        let local = nurbs_surface_local(&scratch, &overflow, 0.0, 0.5).unwrap();
        let actual = polynomial_higher::evaluate(&scratch, &local, polynomial_higher::Orders::ThirdAndFourth).unwrap();
        assert_eq!(actual.fourth(), Err(EvaluationFailure::NonFinite(())));
        assert_eq!(actual.third().unwrap(), [FiniteVector3::ZERO; 4]);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn mixed_polynomial_fourth_survives_first_overflow_in_both_tensor_orientations() {
    let h = 2.0_f64.powi(-260);
    let large = 2.0_f64.powi(1000);
    let small = 2.0_f64.powi(-1000);
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for swapped in [false, true] {
        // S=(A*u/h,v,B*(u/h)^2*v^2), or its u/v transpose.
        // At zero First overflows and uuvv=4*B/h^2=4*2^-480.
        let axis = |width| NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, width, width, width], false);
        let poles = (0..3).map(|i: u32| (0..3).map(|j: u32|
            Point3::new(f64::from(i) / 2.0 * if swapped { 1.0 } else { large },
                f64::from(j) / 2.0 * if swapped { large } else { 1.0 },
                if i == 2 && j == 2 { small } else { 0.0 })
        ).collect()).collect();
        let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
            axis(if swapped { 1.0 } else { h }), axis(if swapped { h } else { 1.0 }),
            NurbsSurfaceLanes::new(poles, None), false).unwrap().unwrap();
        for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
            let scratch = Scratch::new(admission);
            let actual = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
            assert_eq!(actual.jet.point, FinitePoint3::ZERO);
            assert_eq!(actual.jet.first, Err(EvaluationFailure::NonFinite(())));
            assert_eq!(actual.higher.third().unwrap(), [FiniteVector3::ZERO; 4]);
            let third = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Third).unwrap();
            assert_eq!(third.higher.third(), actual.higher.third());
            assert_eq!(third.higher.fourth(), Err(EvaluationFailure::NoValue));
            let fourth = actual.higher.fourth().unwrap();
            assert_eq!(fourth[2].get(), Vector3::new(0.0, 0.0, 4.0 * 2.0_f64.powi(-480)));
            for lane in [0, 1, 3, 4] { assert_eq!(fourth[lane], FiniteVector3::ZERO); }
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn finite_polynomial_third_survives_first_overflow_in_cubic_chart() {
    let width = 2.0_f64.powi(-340);
    let amplitude = 2.0_f64.powi(1000);
    let cubic_amplitude = 2.0_f64.powi(-1000);
    // Exact binary Bernstein coefficients: X_i=A*i give X=3*A*u/h.
    // Y=B*(u/h)^3, so uuu.y=6*B/h^3=6*2^20 at every point.
    let u = NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, width, width, width, width], false);
    let v = NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    let poles = (0..4).map(|i: u32| (0..2).map(|j: u32|
        Point3::new(amplitude * f64::from(i),
            if i == 3 { cubic_amplitude } else { 0.0 }, f64::from(j))
    ).collect()).collect();
    let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), u, v,
        NurbsSurfaceLanes::new(poles, None), false).unwrap().unwrap();
    let mut expected = [FiniteVector3::ZERO; 4];
    expected[0] = FiniteVector3::new(Vector3::new(0.0, 6.0 * 2.0_f64.powi(20), 0.0)).unwrap();
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        let fourth = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, SurfaceRequest::Fourth).unwrap();
        assert_eq!(fourth.jet.point.get(), Point3::new(0.0, 0.0, 0.5));
        assert_eq!(fourth.jet.first, Err(EvaluationFailure::NonFinite(())));
        assert_eq!(fourth.higher.third().unwrap(), expected);
        assert_eq!(fourth.higher.fourth().unwrap(), [FiniteVector3::ZERO; 5]);
        for request in [SurfaceRequest::First, SurfaceRequest::Second, SurfaceRequest::Third] {
            let lower = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, request).unwrap();
            assert_eq!(lower.jet.point, fourth.jet.point);
            assert_eq!(lower.jet.first, fourth.jet.first);
            if request.needs_second() { assert_eq!(lower.jet.second, fourth.jet.second); }
            if request.needs_third() { assert_eq!(lower.higher.third().unwrap(), expected); }
            assert_eq!(lower.higher.fourth(), Err(EvaluationFailure::NoValue));
        }
    }
    ctx.finish_session().unwrap();
}
