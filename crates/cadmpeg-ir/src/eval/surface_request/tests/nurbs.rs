// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::eval::decode::Scratch;
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, WorkBudget};
use cadmpeg_core::CodecError;

const EPS_THIRD_QUOTIENT: f64 = 1.0e-12;

fn cubic() -> NurbsSurface {
    // S=(u,v,u^3+2u^2v+3uv^2+4v^3). Bernstein coefficients
    // for t are i/3; for t^2 they are i(i-1)/6; for t^3, [0,0,0,1].
    let poles = (0..4).map(|i: u32| (0..4).map(|j: u32| {
        let u = f64::from(i) / 3.0;
        let v = f64::from(j) / 3.0;
        let u2 = f64::from(i * i.saturating_sub(1)) / 6.0;
        let v2 = f64::from(j * j.saturating_sub(1)) / 6.0;
        let z = f64::from(u8::from(i == 3)) + 2.0 * u2 * v + 3.0 * u * v2 + 4.0 * f64::from(u8::from(j == 3));
        Point3::new(u, v, z)
    }).collect()).collect();
    let axis = || NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false);
    NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), axis(), axis(), NurbsSurfaceLanes::new(poles, None), false).unwrap().unwrap()
}

#[test]
fn requested_rational_bilinear_third_keeps_nonzero_quotient_corrections() {
    let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), axis(), axis(), NurbsSurfaceLanes::new(
        vec![vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)], vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)]],
        Some(vec![vec![1.0, 1.0], vec![2.0, 2.0]]),
    ), false).unwrap().unwrap();
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let result = crate::eval::nurbs_surface_requested_jet(&scratch, &surface, 0.5, 0.25, SurfaceRequest::Third).unwrap();
    // S=(2u/(1+u),v,0): S_uuu=(12/(1+u)^4,0,0).
    let third = result.higher.third().unwrap();
    assert!((third[0].x - 12.0 / 1.5_f64.powi(4)).abs() <= EPS_THIRD_QUOTIENT);
    assert!(third[0].y.abs() <= EPS_THIRD_QUOTIENT);
    assert!(third[0].z.abs() <= EPS_THIRD_QUOTIENT);
    for lane in &third[1..] {
        assert!(lane.get().norm() <= EPS_THIRD_QUOTIENT);
    }
}

#[test]
fn requested_cubic_mixed_third_and_offset_second_follow_polynomial_coefficients() {
    let surface = cubic();
    let scratch = Scratch::new(EvaluationAdmission::Standard);
    let requested = crate::eval::nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Third).unwrap();
    let third = requested.higher.third().unwrap();
    for (actual, expected) in third.into_iter().zip([6.0, 4.0, 6.0, 24.0]) {
        assert!((actual.z - expected).abs() <= EPS_THIRD_QUOTIENT);
        assert!(actual.x.abs() <= EPS_THIRD_QUOTIENT);
        assert!(actual.y.abs() <= EPS_THIRD_QUOTIENT);
    }
    assert_eq!(requested.jet.second.unwrap(), [crate::features::FiniteVector3::ZERO; 3]);
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "cubic", SolvedSurfaceGeometry::Nurbs(surface));
    let shifted = offset(&mut ir, "offset", base, 1.0);
    let result = evaluate(&ir, &shifted, 0.0, 0.0, SurfaceRequest::Second);
    // At zero S_ij=0, n_i=0 and n_ij=(-z_uij,-z_vij,0).
    assert_eq!(result.point.get(), Point3::new(0.0, 0.0, 1.0));
    for (actual, expected) in result.second.unwrap().into_iter().zip([
        Vector3::new(-6.0, -4.0, 0.0), Vector3::new(-4.0, -6.0, 0.0), Vector3::new(-6.0, -24.0, 0.0),
    ]) {
        assert!((actual.x - expected.x).abs() <= EPS_THIRD_QUOTIENT);
        assert!((actual.y - expected.y).abs() <= EPS_THIRD_QUOTIENT);
        assert!((actual.z - expected.z).abs() <= EPS_THIRD_QUOTIENT);
    }
}

#[test]
fn requested_third_charges_only_real_additional_basis_and_pole_visits() {
    let surface = cubic();
    // Third adds two recurrence rows (2+3+4 each), then four exact
    // sums with one visit per pole. No old homogeneous sum is changed.
    assert_eq!(crate::eval::nurbs_surface_third_evaluation_cost([3, 3]), Some(82));
    assert_eq!(crate::eval::nurbs_surface_third_evaluation_cost([1, 1]), Some(0));
    // The initial five-pass estimate omitted product_sum replay and terminal
    // iterator probes. Retain its 556 cap as a refusal, rather than raise it.
    for request in [SurfaceRequest::Second, SurfaceRequest::Third] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 556;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let Err(EvaluationFailure::ResourceLimit(original)) = crate::eval::nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, request) else {
            panic!("existing homogeneous pole traversal must refuse");
        };
        assert_eq!(original.operation, "IR homogeneous pole traversal");
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!((original.limit, original.used, original.additional), (556, 556, 1));
        assert!(matches!(crate::eval::nurbs_surface_requested_jet(&scratch, &surface, f64::NAN, 0.0, request), Err(EvaluationFailure::ResourceLimit(limit)) if limit == original));
        drop(scratch);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
    // Prepare the actual lower state in the same original decode session.
    // Bound only the real extra stage: 18 basis advances + 64 pole advances.
    for cap in [81, 82] {
        let policy = DecodePolicy::service();
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let scratch = Scratch::new(&ctx);
        let local = crate::eval::nurbs_surface_local(&scratch, &surface, 0.0, 0.0).unwrap();
        let first = local.first(&scratch).unwrap();
        let second = local.second(&scratch, &first).unwrap();
        let budget = ctx.work_budget(cap);
        let result = EvaluationAdmission::Decode(&ctx).within_work_slice(&budget, |admission| {
            let third_scratch = Scratch::new(admission);
            local.third(&third_scratch, &first, &second)
        });
        assert_eq!(budget.consumed(), usize::try_from(cap).unwrap());
        let original = if cap == 81 {
            let Err(EvaluationFailure::ResourceLimit(limit)) = result else {
                panic!("the final actual third pole visit must refuse");
            };
            // work_slice has zero remaining before this one actual visit.
            assert_eq!(limit.dimension, ResourceDimension::Codec("geometry evaluation work slice"));
            assert_eq!((limit.limit, limit.used, limit.additional), (0, 0, 1));
            assert_eq!(limit.operation, "geometry evaluation work slice");
            assert!(matches!(local.third(&scratch, &first, &second), Err(EvaluationFailure::ResourceLimit(sticky)) if sticky == limit));
            Some(limit)
        } else {
            let lanes = result.unwrap();
            for (actual, expected) in lanes.into_iter().zip([6.0, 4.0, 6.0, 24.0]) {
                assert!((actual[2].get() - expected).abs() <= EPS_THIRD_QUOTIENT);
            }
            None
        };
        drop(second);
        drop(first);
        drop(local);
        drop(scratch);
        drop(budget);
        match original {
            Some(original) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original)),
            None => { ctx.finish_session().unwrap(); }
        }
    }
    // Preserve the existing Independent Second cost112; third needs 82
    // actual extra advances. Lower orders survive independent refusal.
    for cap in [193, 194] {
        let budget = WorkBudget::new(cap);
        let result = EvaluationAdmission::Standard.within_work_slice(&budget, |admission| {
            let scratch = Scratch::new(admission);
            crate::eval::nurbs_surface_requested_jet(&scratch, &surface, 0.0, 0.0, SurfaceRequest::Third)
        }).unwrap();
        assert!(result.jet.first.is_ok());
        assert!(result.jet.second.is_ok());
        if cap == 193 {
            assert!(matches!(result.higher.third(), Err(EvaluationFailure::NoValue)));
        } else {
            assert!(result.higher.third().is_ok());
            assert_eq!(budget.consumed(), 194);
        }
    }
}

#[test]
fn requested_cancellation_does_not_compute_nurbs_third() {
    let mut ir = CadIr::empty();
    let base = stored(&mut ir, "cubic", SolvedSurfaceGeometry::Nurbs(cubic()));
    let inner = offset(&mut ir, "inner", base, 1.0);
    let cancelled = offset(&mut ir, "cancelled", inner, -1.0);
    let index = ModelIndex::build(&ir, StandardIndex);
    let result = super::super::with_mapping(EvaluationAdmission::Standard, &index, &cancelled, 0.0, 0.0, &mut |mapping| {
        mapping.evaluate(EvaluationAdmission::Standard, &index, SurfaceRequest::Second)
    }).unwrap();
    assert!(result.jet.second.is_ok());
    assert!(matches!(result.higher, HigherPartials::Third(Err(EvaluationFailure::NoValue))));
}
