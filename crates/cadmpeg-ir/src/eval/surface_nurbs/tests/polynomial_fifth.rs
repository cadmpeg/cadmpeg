// SPDX-License-Identifier: Apache-2.0
use super::*;

// Stored binary64 poles i/5: 120*sum((-1)^(5-i)*C(5,i)*X_i).
// X_1=3602879701896397/2^54, X_2=3602879701896397/2^53,
// X_3=5404319552844595/2^53, X_4=3602879701896397/2^52.
const QUINTIC_LINEAR_FIFTH: f64 = -660.0 / 9_007_199_254_740_992.0;

fn monomial(u_degree: u32, v_degree: u32, amplitude: f64, u_width: f64) -> NurbsSurface {
    let axis = |degree, width| NurbsSurfaceAxis::new(degree,
        std::iter::repeat_n(0.0, usize::try_from(degree + 1).unwrap())
            .chain(std::iter::repeat_n(width, usize::try_from(degree + 1).unwrap())).collect::<Vec<_>>(), false);
    let poles = (0..=u_degree).map(|i| (0..=v_degree).map(|j|
        Point3::new(f64::from(i) / f64::from(u_degree), f64::from(j) / f64::from(v_degree),
            if i == u_degree && j == v_degree { amplitude } else { 0.0 })
    ).collect()).collect();
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(),
        axis(u_degree, u_width), axis(v_degree, 1.0), NurbsSurfaceLanes::new(poles, None), false).unwrap().unwrap()
}

fn quintic_graph(reverse: bool, amplitude: f64) -> NurbsSurface {
    let [u_degree, v_degree] = if reverse { [1, 5] } else { [5, 1] };
    let axis = |degree| NurbsSurfaceAxis::new(degree,
        std::iter::repeat_n(0.0, usize::try_from(degree + 1).unwrap())
            .chain(std::iter::repeat_n(1.0, usize::try_from(degree + 1).unwrap())).collect::<Vec<_>>(), false);
    // The height is u^5 or v^5. The linear coordinate poles i/5
    // are rounded binary64 values, with the exact fifth sum above.
    let poles = (0..=u_degree).map(|i| (0..=v_degree).map(|j|
        Point3::new(f64::from(i) / f64::from(u_degree), f64::from(j) / f64::from(v_degree),
            if (if reverse { j } else { i }) == 5 { amplitude } else { 0.0 })
    ).collect()).collect();
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), axis(u_degree), axis(v_degree),
        NurbsSurfaceLanes::new(poles, None), false).unwrap().unwrap()
}

#[test]
fn actual_polynomial_fifth_uses_all_monomial_mixed_derivatives_and_preserves_lower_orders() {
    let surfaces = [(quintic_graph(false, 1.0), 0, 120.0), (monomial(4, 1, 1.0, 1.0), 1, 24.0),
        (monomial(3, 2, 1.0, 1.0), 2, 12.0), (monomial(2, 3, 1.0, 1.0), 3, 12.0),
        (monomial(1, 4, 1.0, 1.0), 4, 24.0), (quintic_graph(true, 1.0), 5, 120.0)];
    let policy = DecodePolicy::service();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        for (surface, selected, expected) in &surfaces {
            let actual = nurbs_surface_requested_jet(&scratch, surface, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
            for (order, vector) in actual.higher.fifth().unwrap().into_iter().enumerate() {
                close(vector.get(), Vector3::new(0.0, 0.0, if order == *selected { *expected } else { 0.0 }));
            }
            let lower = nurbs_surface_requested_jet(&scratch, surface, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
            assert_eq!(actual.jet.point, lower.jet.point);
            assert_eq!(actual.jet.first, lower.jet.first);
            assert_eq!(actual.jet.second, lower.jet.second);
            assert_eq!(actual.higher.third(), lower.higher.third());
            assert_eq!(actual.higher.fourth(), lower.higher.fourth());
            assert_eq!(lower.higher.fifth(), Err(EvaluationFailure::NoValue));
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn actual_polynomial_fifth_projection_retains_extended_span_range() {
    let surface = monomial(5, 1, 2.0_f64.powi(-1000), 2.0_f64.powi(-210));
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    // z=2^-1000*(u/2^-210)^5*v, so uuuuu.z=120*2^50*v.
    let actual = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.5, SurfaceRequest::Fifth).unwrap();
    // The stored X fifth also divides by h^5: -660*2^-53*2^1050.
    assert_eq!(actual.higher.fifth().unwrap()[0].get(), Vector3::new(-660.0 * 2.0_f64.powi(997), 0.0, 60.0 * 2.0_f64.powi(50)));
    for lane in &actual.higher.fifth().unwrap()[1..] { assert_eq!(*lane, FiniteVector3::ZERO); }
}

#[test]
fn polynomial_fifth_real_49_steps_preserve_standard_and_decode_refusal() {
    let surface = quintic_graph(false, 1.0);
    // Support6:12 initializations +5 rows +20 cells; one support12 walk.
    for cap in [48, 49] {
        let policy = DecodePolicy::service();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let local = nurbs_surface_local(&scratch, &surface, 0.0, 0.0).unwrap();
        let standard = WorkBudget::new(cap);
        let actual = EvaluationAdmission::Standard.within_work_slice(&standard, |admission|
            higher::fifth::evaluate(&Scratch::new(admission), &local));
        assert_eq!(standard.consumed(), cap);
        if cap == 48 { assert_eq!(actual, Err(EvaluationFailure::NoValue)); }
        else { assert_eq!(actual.unwrap()[0].get(), Vector3::new(QUINTIC_LINEAR_FIFTH, 0.0, 120.0)); }
        let budget = ctx.work_budget(u64::try_from(cap).unwrap());
        let actual = EvaluationAdmission::Decode(&ctx).within_work_slice(&budget, |admission|
            higher::fifth::evaluate(&Scratch::new(admission), &local));
        assert_eq!(budget.consumed(), cap);
        let original = if cap == 48 {
            let Err(EvaluationFailure::ResourceLimit(limit)) = actual else { panic!("last fifth pole must refuse"); };
            assert_eq!(limit.dimension, ResourceDimension::Codec("geometry evaluation work slice"));
            assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
            assert_eq!(limit.operation, "geometry evaluation work slice");
            assert!(matches!(higher::fifth::evaluate(&scratch, &local), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == limit));
            Some(limit)
        } else { assert_eq!(actual.unwrap()[0].get(), Vector3::new(QUINTIC_LINEAR_FIFTH, 0.0, 120.0)); None };
        drop(local); drop(scratch); drop(budget);
        match original {
            Some(limit) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)),
            None => { ctx.finish_session().unwrap(); }
        }
    }
}

#[test]
fn actual_fifth_overflow_keeps_completed_lower_orders() {
    let surface = quintic_graph(false, f64::MAX);
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let actual = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Fifth).unwrap();
    let lower = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
    assert_eq!(actual.jet.point, lower.jet.point);
    assert_eq!(actual.jet.first, lower.jet.first);
    assert_eq!(actual.jet.second, lower.jet.second);
    assert_eq!(actual.higher.third(), lower.higher.third());
    assert_eq!(actual.higher.fourth(), lower.higher.fourth());
    assert_eq!(actual.higher.fifth(), Err(EvaluationFailure::NonFinite(())));
}
