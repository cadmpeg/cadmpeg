// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::admission::EvaluationAdmission;
use crate::eval::surface_request::SurfaceRequest;
use crate::math::{Point3, Vector3};
use crate::eval::decode::Scratch;
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget};
use cadmpeg_core::CodecError;

const EPS_NURBS_FOURTH: f64 = 1.0e-12;

fn close(actual: Vector3, expected: Vector3) {
    assert!((actual - expected).norm() <= EPS_NURBS_FOURTH, "{actual:?} vs {expected:?}");
}

pub(in crate::eval) fn quartic() -> NurbsSurface {
    // Bernstein coefficients for t^k are C(i,k)/C(4,k).
    let coefficients = [
        [1.0, 0.0, 0.0, 0.0, 0.0],
        [1.0, 0.25, 0.0, 0.0, 0.0],
        [1.0, 0.5, 1.0 / 6.0, 0.0, 0.0],
        [1.0, 0.75, 0.5, 0.25, 0.0],
        [1.0, 1.0, 1.0, 1.0, 1.0],
    ];
    let poles = coefficients.iter().map(|u| coefficients.iter().map(|v| {
        Point3::new(u[1], v[1], u[4] + 2.0 * u[3] * v[1]
            + 3.0 * u[2] * v[2] + 4.0 * u[1] * v[3] + 5.0 * v[4])
    }).collect()).collect();
    let axis = || NurbsSurfaceAxis::new(4,
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0], false);
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), axis(), axis(),
        NurbsSurfaceLanes::new(poles, None), false).unwrap().unwrap()
}

fn bilinear(poles: Vec<Vec<Point3>>, weights: Vec<Vec<f64>>) -> NurbsSurface {
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), axis(), axis(),
        NurbsSurfaceLanes::new(poles, Some(weights)), false).unwrap().unwrap()
}

#[test]
fn actual_quartic_fourth_follows_full_polynomial_laws() {
    let surface = quartic();
    for (u, v) in [(0.0, 0.0), (0.3, 0.4)] {
        let scratch = Scratch::new(EvaluationAdmission::Standard);
        let result = crate::eval::surface_nurbs::nurbs_surface_requested_jet(&scratch, &surface, u, v, SurfaceRequest::Fourth).unwrap();
        for (actual, expected) in result.higher.fourth().unwrap().into_iter().zip([24.0, 12.0, 12.0, 24.0, 120.0]) {
            close(actual.get(), Vector3::new(0.0, 0.0, expected));
        }
        let lower = crate::eval::surface_nurbs::nurbs_surface_requested_jet(&scratch, &surface, u, v, SurfaceRequest::Third).unwrap();
        assert_eq!(result.jet.point, lower.jet.point);
        assert_eq!(result.jet.first, lower.jet.first);
        assert_eq!(result.jet.second, lower.jet.second);
        assert_eq!(result.higher.third(), lower.higher.third());
    }

}

#[test]
fn actual_rational_bilinear_fourth_keeps_pure_and_mixed_quotient_terms() {
    let pure = bilinear(
        vec![vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)]],
        vec![vec![1.0, 1.0], vec![2.0, 2.0]],
    );
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let result = crate::eval::surface_nurbs::nurbs_surface_requested_jet(&scratch, &pure, 0.5, 0.25, SurfaceRequest::Fourth).unwrap();
    let fourth = result.higher.fourth().unwrap();
    close(fourth[0].get(), Vector3::new(-48.0 / 1.5_f64.powi(5), 0.0, 0.0));
    for actual in &fourth[1..] { close(actual.get(), Vector3::new(0.0, 0.0, 0.0)); }

    // H=(u,v,0), W=1+u+v. The order-four Taylor term is
    // -(u,v,0)*(u+v)^3; differentiate its exact monomials.
    let mixed = bilinear(
        vec![vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.5, 0.0)],
            vec![Point3::new(0.5, 0.0, 0.0), Point3::new(1.0 / 3.0, 1.0 / 3.0, 0.0)]],
        vec![vec![1.0, 2.0], vec![2.0, 3.0]],
    );
    let result = crate::eval::surface_nurbs::nurbs_surface_requested_jet(&scratch, &mixed, 0.0, 0.0, SurfaceRequest::Fourth).unwrap();
    for (actual, (x, y)) in result.higher.fourth().unwrap().into_iter().zip([
        (-24.0, 0.0), (-18.0, -6.0), (-12.0, -12.0), (-6.0, -18.0), (0.0, -24.0),
    ]) { close(actual.get(), Vector3::new(x, y, 0.0)); }
}

#[test]
fn actual_fourth_reuses_third_state_with_exact_additional_pole_visits() {
    // A cubic tensor representation of (u,v,0) has the same real degree,
    // row and pole advances as the original nonzero cubic derivative control.
    let axis = || NurbsSurfaceAxis::new(3,
        vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false);
    let poles = (0..4).map(|i: u32| (0..4).map(|j: u32|
        Point3::new(f64::from(i) / 3.0, f64::from(j) / 3.0, 0.0)
    ).collect()).collect();
    let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), axis(), axis(),
        NurbsSurfaceLanes::new(poles, None), false).unwrap().unwrap();
    assert_eq!(crate::eval::surface_nurbs::fourth_evaluation_cost([3, 3]), Some(48));
    assert_eq!(crate::eval::surface_nurbs::fourth_evaluation_cost([4, 4]), Some(153));
    assert_eq!(crate::eval::surface_nurbs::fourth_evaluation_cost([1, 1]), Some(0));
    for cap in [47, 48] {
        let policy = DecodePolicy::service();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let local = crate::eval::surface_nurbs::nurbs_surface_local(&scratch, &surface, 0.0, 0.0).unwrap();
        let first = local.first(&scratch).unwrap();
        let second = local.second(&scratch, &first).unwrap();
        let third = local.third(&scratch, &first, &second).unwrap();
        let budget = ctx.work_budget(cap);
        let result = EvaluationAdmission::Decode(&ctx).within_work_slice(&budget, |admission| {
            let fourth_scratch = Scratch::new(admission);
            local.fourth(&fourth_scratch, &first, &second, &third)
        });
        assert_eq!(budget.consumed(), usize::try_from(cap).unwrap());
        let original = if cap == 47 {
            let Err(EvaluationFailure::ResourceLimit(limit)) = result else { panic!("last fourth pole visit must refuse"); };
            assert_eq!(limit.dimension, ResourceDimension::Codec("geometry evaluation work slice"));
            assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
            assert_eq!(limit.operation, "geometry evaluation work slice");
            assert!(matches!(local.fourth(&scratch, &first, &second, &third), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == limit));
            Some(limit)
        } else {
            for lane in result.unwrap() { assert!(lane.iter().all(|value| value.get().abs() <= EPS_NURBS_FOURTH)); }
            None
        };
        drop(third); drop(second); drop(first); drop(local); drop(scratch); drop(budget);
        match original {
            Some(limit) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit)),
            None => { ctx.finish_session().unwrap(); }
        }
    }
    for cap in [241, 242] {
        let budget = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&budget, |admission| {
            let scratch = Scratch::new(admission);
            crate::eval::surface_nurbs::nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Fourth)
        }).unwrap();
        assert!(result.jet.first.is_ok() && result.jet.second.is_ok() && result.higher.third().is_ok());
        if cap == 241 { assert_eq!(result.higher.fourth(), Err(EvaluationFailure::NoValue)); }
        else { assert!(result.higher.fourth().is_ok()); assert_eq!(budget.consumed(), 242); }
    }
}

mod polynomial_zero;
mod polynomial_extended;
