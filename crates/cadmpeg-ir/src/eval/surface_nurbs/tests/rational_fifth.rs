// SPDX-License-Identifier: Apache-2.0
use super::*;

fn quintic(amplitude: f64) -> NurbsSurface {
    let u = NurbsSurfaceAxis::new(5, vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
        1.0, 1.0, 1.0, 1.0, 1.0, 1.0], false);
    let v = NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    // X=5u uses exact integer poles. Y=B*u^5; Z=2v/(1+v).
    let poles = (0..6).map(|i: u32| (0..2).map(|j: u32|
        Point3::new(f64::from(i), if i == 5 { amplitude } else { 0.0 }, f64::from(j))
    ).collect()).collect();
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), u, v,
        NurbsSurfaceLanes::new(poles, Some(vec![vec![1.0, 2.0]; 6])), false).unwrap().unwrap()
}

#[test]
fn actual_rational_fifth_keeps_all_mixed_laws_and_completed_lower_orders() {
    // H=(u,v,0), W=1+u+v: fifth Taylor term (u,v,0)*(u+v)^4.
    let surface = bilinear(
        vec![vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.5, 0.0)],
            vec![Point3::new(0.5, 0.0, 0.0), Point3::new(1.0 / 3.0, 1.0 / 3.0, 0.0)]],
        vec![vec![1.0, 2.0], vec![2.0, 3.0]],
    );
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        let actual = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
        for (actual, (x, y)) in actual.higher.fifth().unwrap().into_iter().zip([
            (120.0, 0.0), (96.0, 24.0), (72.0, 48.0), (48.0, 72.0), (24.0, 96.0), (0.0, 120.0),
        ]) { close(actual.get(), Vector3::new(x, y, 0.0)); }
        let lower = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
        assert_eq!(actual.jet.point, lower.jet.point);
        assert_eq!(actual.jet.first, lower.jet.first); assert_eq!(actual.jet.second, lower.jet.second);
        assert_eq!(actual.higher.third(), lower.higher.third()); assert_eq!(actual.higher.fourth(), lower.higher.fourth());
        assert_eq!(lower.higher.fifth(), Err(EvaluationFailure::NoValue));
        // The real normalized owner remains usable after first-order overflow.
        let small = polynomial_extended::small_span(true, 2.0_f64.powi(-1000));
        let actual = nurbs_surface_requested_jet(&scratch, &small, 0.0, 0.5, SurfaceRequest::Fifth).unwrap();
        assert_eq!(actual.jet.first, Err(EvaluationFailure::NonFinite(())));
        for lane in &actual.higher.fifth().unwrap()[..5] { assert_eq!(*lane, FiniteVector3::ZERO); }
        close(actual.higher.fifth().unwrap()[5].get(), Vector3::new(0.0, 0.0, 240.0 / 1.5_f64.powi(6)));
        let lower = nurbs_surface_requested_jet(&scratch, &small, 0.0, 0.5, SurfaceRequest::Fourth).unwrap();
        assert_eq!(actual.jet.point, lower.jet.point); assert_eq!(actual.jet.second, lower.jet.second);
        assert_eq!(actual.higher.third(), lower.higher.third()); assert_eq!(actual.higher.fourth(), lower.higher.fourth());
    }
    ctx.finish_session().unwrap();
}

#[test]
fn rational_fifth_actual_49_step_boundary_keeps_standard_and_original_decode_fuse() {
    let surface = quintic(1.0);
    // Support6:12 initializations,5 rows,20 cells,one12-pole raw walk.
    for cap in [48, 49] {
        let policy = DecodePolicy::service(); let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let local = nurbs_surface_local(&scratch, &surface, 0.0, 0.0).unwrap();
        let standard = WorkBudget::new(cap);
        let actual = EvaluationAdmission::Standard.within_work_slice(&standard, |admission|
            higher::fifth::evaluate(&Scratch::new(admission), &local));
        assert_eq!(standard.consumed(), cap);
        if cap == 48 { assert_eq!(actual, Err(EvaluationFailure::NoValue)); }
        else {
            let actual = actual.unwrap();
            assert_eq!(actual[0].get(), Vector3::new(0.0, 120.0, 0.0));
            assert_eq!(actual[5].get(), Vector3::new(0.0, 0.0, 240.0));
            for lane in &actual[1..5] { assert_eq!(*lane, FiniteVector3::ZERO); }
        }
        let budget = ctx.work_budget(u64::try_from(cap).unwrap());
        let actual = EvaluationAdmission::Decode(&ctx).within_work_slice(&budget, |admission|
            higher::fifth::evaluate(&Scratch::new(admission), &local));
        assert_eq!(budget.consumed(), cap);
        let original = if cap == 48 {
            let Err(EvaluationFailure::ResourceLimit(limit)) = actual else { panic!("last real Rational pole must refuse"); };
            assert_eq!(limit.dimension, ResourceDimension::Codec("geometry evaluation work slice"));
            assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
            assert_eq!(limit.operation, "geometry evaluation work slice");
            assert!(matches!(higher::fifth::evaluate(&scratch, &local), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == limit));
            Some(limit)
        } else {
            let actual = actual.unwrap();
            assert_eq!(actual[0].get(), Vector3::new(0.0, 120.0, 0.0));
            assert_eq!(actual[5].get(), Vector3::new(0.0, 0.0, 240.0)); None
        };
        drop(local); drop(scratch); drop(budget);
        match original {
            Some(limit) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)),
            None => { ctx.finish_session().unwrap(); }
        }
    }
}

#[test]
fn rational_fifth_lost_fourth_basis_on_constant_other_axis_reports_no_value() {
    let h = 2.0_f64.powi(-400);
    let u = NurbsSurfaceAxis::new(4, vec![0.0, 0.0, 0.0, 0.0, 0.0,
        h, 1.0, 2.0, 3.0, 4.0, 4.0, 4.0, 4.0, 4.0], false);
    let v = NurbsSurfaceAxis::new(0, vec![0.0, 1.0], false);
    let poles = (0..9).map(|i| vec![Point3::new(if i == 4 { 1.0 } else { 0.0 }, 0.0, 0.0)]).collect();
    let weights = (0..9).map(|i| vec![if i == 1 { 2.0 } else { 1.0 }]).collect();
    let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), u, v,
        NurbsSurfaceLanes::new(poles, Some(weights)), false).unwrap().unwrap();
    // B4=t^4/(6h): normalized D4=4h^3 is lost. Rational D5 needs D4
    // through the nonconstant weight even though the other degree is zero.
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let local = nurbs_surface_local(&scratch, &surface, 0.0, 0.5).unwrap();
    assert_eq!(higher::fifth::evaluate(&scratch, &local), Err(EvaluationFailure::NoValue));
}

#[test]
fn rational_fifth_constant_coordinate_and_true_overflow_keep_lower_results() {
    let constant = bilinear(vec![vec![Point3::new(f64::MAX, f64::MAX, f64::MAX); 2]; 2],
        vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let local = nurbs_surface_local(&scratch, &constant, 0.0, 0.0).unwrap();
    assert_eq!(higher::fifth::evaluate(&scratch, &local), Ok([FiniteVector3::ZERO; 6]));
    let overflow = quintic(f64::MAX);
    let actual = nurbs_surface_requested_jet(&scratch, &overflow, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
    let lower = nurbs_surface_requested_jet(&scratch, &overflow, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
    assert_eq!(actual.jet.point, lower.jet.point); assert_eq!(actual.jet.first, lower.jet.first);
    assert_eq!(actual.jet.second, lower.jet.second); assert_eq!(actual.higher.third(), lower.higher.third());
    assert_eq!(actual.higher.fourth(), lower.higher.fourth());
    assert_eq!(actual.higher.fifth(), Err(EvaluationFailure::NonFinite(())));
}
