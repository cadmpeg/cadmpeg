// SPDX-License-Identifier: Apache-2.0
use super::*;

fn polynomial(u_degree: u32, v_degree: u32, poles: Vec<Vec<Point3>>) -> NurbsSurface {
    let axis = |degree| NurbsSurfaceAxis::new(degree,
        std::iter::repeat_n(0.0, usize::try_from(degree + 1).unwrap())
            .chain(std::iter::repeat_n(1.0, usize::try_from(degree + 1).unwrap())).collect::<Vec<_>>(), false);
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), axis(u_degree), axis(v_degree),
        NurbsSurfaceLanes::new(poles, None), false).unwrap().unwrap()
}

fn overflow_bilinear() -> NurbsSurface {
    // S=(u,v,MAX*(2u-1)*(2v-1)). At(.5,.5), point/First are
    // finite, S_uv=4MAX is not, and every Third/Fourth is exactly zero.
    polynomial(1, 1, vec![
        vec![Point3::new(0.0, 0.0, f64::MAX), Point3::new(0.0, 1.0, -f64::MAX)],
        vec![Point3::new(1.0, 0.0, -f64::MAX), Point3::new(1.0, 1.0, f64::MAX)],
    ])
}

#[test]
fn stored_polynomial_surface_zero_orders_survive_lower_overflow_without_affine_inference() {
    let bilinear = overflow_bilinear();
    // S=(u,v,MAX*u^2*v). At(0,.5), S_uu=MAX is finite,
    // S_uuv=2MAX is not, and every Fourth is exactly zero.
    let cubic = polynomial(2, 1, (0..3).map(|i: u32| (0..2).map(|j: u32|
        Point3::new(f64::from(i) / 2.0, f64::from(j),
            if i == 2 && j == 1 { f64::MAX } else { 0.0 })
    ).collect()).collect());
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        for (surface, u, zero_third) in [(&bilinear, 0.5, true), (&cubic, 0.0, false)] {
            let actual = nurbs_surface_requested_jet(&scratch, surface, u, 0.5, SurfaceRequest::Fourth).unwrap();
            let lower = nurbs_surface_requested_jet(&scratch, surface, u, 0.5, SurfaceRequest::Second).unwrap();
            assert_eq!(actual.jet.point, lower.jet.point);
            assert_eq!(actual.jet.first, lower.jet.first); assert_eq!(actual.jet.second, lower.jet.second);
            assert_eq!(actual.jet.point.get(), Point3::new(u, 0.5, 0.0));
            assert_eq!(actual.jet.first.unwrap().map(FiniteVector3::get), [
                Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0),
            ]);
            if zero_third {
                assert_eq!(actual.jet.second, Err(EvaluationFailure::NonFinite(())));
                assert_eq!(actual.higher.third(), Ok([FiniteVector3::ZERO; 4]));
            } else {
                assert_eq!(actual.jet.second.unwrap()[0].z, f64::MAX);
                assert_eq!(actual.higher.third(), Err(EvaluationFailure::NonFinite(())));
            }
            assert_eq!(actual.higher.fourth(), Ok([FiniteVector3::ZERO; 5]));
            assert!(matches!(actual.higher, HigherPartials::Fourth { .. }));
            for request in [SurfaceRequest::First, SurfaceRequest::Second, SurfaceRequest::Third] {
                assert_eq!(nurbs_surface_requested_jet(&scratch, surface, u, 0.5, request)
                    .unwrap().higher.fourth(), Err(EvaluationFailure::NoValue));
            }
        }
    }
    ctx.finish_session().unwrap();
}

#[test]
fn polynomial_surface_zero_orders_keep_original_work_cost_and_sticky_real_row_refusal() {
    let surface = overflow_bilinear();
    // Existing independent cost: support4 + 3*(2^2+2^2)=28.
    for cap in [27, 28] {
        let budget = WorkBudget::new(cap);
        let actual = EvaluationAdmission::Standard.within_work_slice(&budget, |admission|
            nurbs_surface_requested_jet(&Scratch::new(admission), &surface, 0.5, 0.5, SurfaceRequest::Fourth));
        if cap == 27 { assert!(matches!(actual, Err(EvaluationFailure::NoValue))); }
        else {
            let actual = actual.unwrap(); assert_eq!(actual.higher.third(), Ok([FiniteVector3::ZERO; 4]));
            assert_eq!(actual.higher.fourth(), Ok([FiniteVector3::ZERO; 5]));
        }
        assert_eq!(budget.consumed(), cap);
    }
    let mut policy = DecodePolicy::service(); policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let scratch = Scratch::new(&ctx);
    // Degree1 bases and their two fixed lanes use no variable work. The
    // four-pole base sum visits support before any derivative row.
    let Err(EvaluationFailure::ResourceLimit(original)) =
        nurbs_surface_requested_jet(&scratch, &surface, 0.5, 0.5, SurfaceRequest::Second)
        else { panic!("actual base-point support traversal must refuse"); };
    assert_eq!(original.operation, "IR homogeneous pole traversal");
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert!(matches!(nurbs_surface_requested_jet(&scratch, &surface, f64::NAN, 0.5, SurfaceRequest::Fourth),
        Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == original));
    drop(scratch);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == original));
}

#[test]
fn a_zero_sampled_third_does_not_erase_true_mixed_polynomial_fourth() {
    let surface = polynomial(2, 2, (0..3).map(|i: u32| (0..3).map(|j: u32|
        Point3::new(f64::from(i) / 2.0, f64::from(j) / 2.0,
            if i == 2 && j == 2 { 1.0 } else { 0.0 })
    ).collect()).collect());
    let mut policy = DecodePolicy::service(); policy.limits.max_retained_bytes = 0;
    let arena = DecodeArena::new(); let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for admission in [EvaluationAdmission::Standard, EvaluationAdmission::Decode(&ctx)] {
        let scratch = Scratch::new(admission);
        let actual = nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
        assert_eq!(actual.higher.third(), Ok([FiniteVector3::ZERO; 4]));
        let fourth = actual.higher.fourth().unwrap();
        assert_eq!(fourth[2].get(), Vector3::new(0.0, 0.0, 4.0));
        for lane in [0, 1, 3, 4] { assert_eq!(fourth[lane], FiniteVector3::ZERO); }
    }
    ctx.finish_session().unwrap();
}
